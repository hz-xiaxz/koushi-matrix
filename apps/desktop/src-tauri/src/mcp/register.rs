//! Runs an MCP client's own registration CLI on the user's request.
//!
//! The webview names only a client; the argument vector comes from
//! `koushi_mcp_host::registration::command_argv` and the launcher path the app
//! resolved itself. Koushi writes no file of the client: the client's CLI
//! does. A GUI app does not inherit the shell's `PATH`, and these CLIs are
//! often Node scripts that need `node` on `PATH` too, so the program is looked
//! up, and the child runs, with the user's login `PATH`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use koushi_mcp_host::registration::{RegistrationClient, command_argv};
use serde::Serialize;
use tokio::io::AsyncReadExt;

/// How long the client's CLI may run.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the login shell may take to report its `PATH`.
#[cfg(unix)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Output kept from each of stdout and stderr.
const OUTPUT_CAP: usize = 4 * 1024;

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RegisterOutcome {
    Registered {
        output: String,
    },
    /// The client's CLI is not installed, or not on the login `PATH`.
    NotFound {
        program: String,
    },
    Failed {
        exit_code: Option<i32>,
        output: String,
    },
    TimedOut,
    /// Another registration is running.
    InProgress,
    /// Windows refused to pass an argument to a batch-file shim safely.
    UnsafeArguments,
    LauncherUnavailable,
    /// The client has no registration CLI.
    Unsupported,
}

impl RegisterOutcome {
    /// The closed outcome code, for diagnostics.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Registered { .. } => "registered",
            Self::NotFound { .. } => "not_found",
            Self::Failed { .. } => "failed",
            Self::TimedOut => "timed_out",
            Self::InProgress => "in_progress",
            Self::UnsafeArguments => "unsafe_arguments",
            Self::LauncherUnavailable => "launcher_unavailable",
            Self::Unsupported => "unsupported",
        }
    }
}

impl std::fmt::Debug for RegisterOutcome {
    // The client's output may name local paths; it stays on screen only.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.kind())
    }
}

/// Runs the registration command of `client` for `launcher`.
pub(crate) async fn register(client: RegistrationClient, launcher: &str) -> RegisterOutcome {
    let Some((program, args)) = command_argv(client, launcher) else {
        return RegisterOutcome::Unsupported;
    };
    let dirs = search_dirs().await;
    let Some(resolved) = resolve_program(program, &dirs) else {
        return RegisterOutcome::NotFound {
            program: program.to_owned(),
        };
    };
    run(&resolved, &args, &dirs, RUN_TIMEOUT).await
}

/// Runs `program` with `args` under `path_dirs`, capturing bounded output.
pub(crate) async fn run(
    program: &Path,
    args: &[String],
    path_dirs: &[PathBuf],
    timeout: Duration,
) -> RegisterOutcome {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    if let Ok(joined) = std::env::join_paths(path_dirs) {
        command.env("PATH", joined);
    }
    if let Some(home) = home_dir() {
        command.current_dir(home);
    }
    strip_appimage_environment(&mut command);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = match spawn_retrying_busy(&mut command).await {
        Ok(child) => child,
        // Rust refuses arguments it cannot pass to a .cmd/.bat shim safely.
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
            return RegisterOutcome::UnsafeArguments;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RegisterOutcome::NotFound {
                program: program.to_string_lossy().into_owned(),
            };
        }
        Err(_) => {
            return RegisterOutcome::Failed {
                exit_code: None,
                output: String::new(),
            };
        }
    };
    // The CLI leads its own process group; its pid is the group id.
    let group = child.id();
    let out = Captured::default();
    let err = Captured::default();
    let stdout = tokio::spawn(read_capped(child.stdout.take(), out.clone()));
    let stderr = tokio::spawn(read_capped(child.stderr.take(), err.clone()));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        Ok(Err(_)) => None,
        Err(_) => {
            kill_tree(&mut child).await;
            stdout.abort();
            stderr.abort();
            return RegisterOutcome::TimedOut;
        }
    };
    // Whatever the CLI left running in its group (an update checker, a
    // daemon holding the pipes) goes with it.
    kill_group_remainder(group);
    for reader in [stdout, stderr] {
        let mut reader = reader;
        if tokio::time::timeout(Duration::from_secs(2), &mut reader)
            .await
            .is_err()
        {
            reader.abort();
        }
    }
    let output = clean_output(&[out.take(), err.take()].concat());
    match status {
        Some(status) if status.success() => RegisterOutcome::Registered { output },
        Some(status) => RegisterOutcome::Failed {
            exit_code: status.code(),
            output,
        },
        None => RegisterOutcome::Failed {
            exit_code: None,
            output,
        },
    }
}

/// Output read so far; a reader stopped at the deadline keeps what it read.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl Captured {
    fn push(&self, bytes: &[u8]) {
        let mut kept = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let room = OUTPUT_CAP.saturating_sub(kept.len());
        kept.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }

    fn take(&self) -> Vec<u8> {
        std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// Spawns, retrying briefly while the executable is busy (`ETXTBSY`): a file
/// that another process still holds open for writing (a package manager
/// updating the CLI, or a fork that inherited a write handle) cannot be
/// executed for that moment.
async fn spawn_retrying_busy(
    command: &mut tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 10 =>
            {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            result => return result,
        }
    }
}

/// Reads a pipe to its end, keeping at most [`OUTPUT_CAP`] bytes and draining
/// the rest so the child never blocks on a full pipe.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>, into: Captured) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut chunk = [0u8; 1024];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => into.push(&chunk[..read]),
        }
    }
}

/// After the CLI exited: kills what remains of its process group. The group
/// id is the CLI's pid, which cannot be reused while group members exist.
#[cfg(unix)]
fn kill_group_remainder(group: Option<u32>) {
    if let Some(group) = group {
        // SAFETY: killpg only sends a signal; an empty group fails harmlessly.
        unsafe {
            libc::killpg(group as libc::pid_t, libc::SIGKILL);
        }
    }
}

/// Windows has no group to signal once the parent is gone; the readers are
/// stopped at their deadline instead.
#[cfg(windows)]
fn kill_group_remainder(_group: Option<u32>) {}

#[cfg(unix)]
async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        // The child leads its own process group (`process_group(0)`).
        // SAFETY: killpg only sends a signal; a stale group id fails harmlessly.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
}

#[cfg(windows)]
async fn kill_tree(child: &mut tokio::process::Child) {
    // Killing a .cmd shim's cmd.exe would leave node.exe running.
    if let Some(pid) = child.id() {
        let _ = tokio::process::Command::new(system32("taskkill.exe"))
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = child.kill().await;
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A system tool by full path, so a same-named file next to the app or on
/// `PATH` is never run instead.
#[cfg(windows)]
fn system32(tool: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    PathBuf::from(root).join("System32").join(tool)
}

/// Variables an AppImage sets to point into its mount; a child that inherits
/// them (for example `node`) may load the wrong libraries.
fn strip_appimage_environment(command: &mut tokio::process::Command) {
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }
    for name in [
        "APPDIR",
        "APPIMAGE",
        "ARGV0",
        "OWD",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "GIO_MODULE_DIR",
        "GDK_PIXBUF_MODULE_FILE",
        "GDK_PIXBUF_MODULEDIR",
        "GSETTINGS_SCHEMA_DIR",
        "GTK_PATH",
        "GTK_EXE_PREFIX",
        "GTK_DATA_PREFIX",
        "GTK_IM_MODULE_FILE",
        "PYTHONHOME",
        "PYTHONPATH",
        "PERLLIB",
        "QT_PLUGIN_PATH",
    ] {
        command.env_remove(name);
    }
}

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Directories to search, in order: the login `PATH` (or, on Windows, the
/// registry `Path`), common install directories, then the app's own `PATH`.
/// Only absolute entries are kept, so `.` or an empty entry cannot resolve
/// against Koushi's working directory.
async fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(unix)]
    if let Some(path) = login_shell_path().await {
        dirs.extend(std::env::split_paths(&path));
    }
    #[cfg(windows)]
    dirs.extend(windows_registry_path().await);
    dirs.extend(fallback_dirs());
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    absolute_unique(dirs)
}

pub(crate) fn absolute_unique(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    for dir in dirs {
        if dir.is_absolute() && !seen.contains(&dir) {
            seen.push(dir);
        }
    }
    seen
}

fn fallback_dirs() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    #[cfg(unix)]
    {
        let mut dirs: Vec<PathBuf> = [
            ".local/bin",
            ".npm-global/bin",
            ".volta/bin",
            ".asdf/shims",
            ".bun/bin",
            ".cargo/bin",
            ".claude/local",
        ]
        .iter()
        .map(|relative| home.join(relative))
        .collect();
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs
    }
    #[cfg(windows)]
    {
        let mut dirs = vec![home.join(".local").join("bin")];
        if let Some(app_data) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(app_data).join("npm"));
        }
        dirs
    }
}

/// Finds `program` in `dirs`. On Windows each directory is searched over
/// `PATHEXT`, `.exe` first, so a real executable wins over a batch shim.
pub(crate) fn resolve_program(program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in dirs {
        #[cfg(unix)]
        {
            let candidate = dir.join(program);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        #[cfg(windows)]
        for extension in windows_extensions() {
            let candidate = dir.join(format!("{program}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn windows_extensions() -> Vec<String> {
    let listed = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
    let mut extensions: Vec<String> = listed
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    extensions.sort_by_key(|extension| extension != ".exe");
    extensions
}

/// Asks the user's login shell for its `PATH`. The shell runs interactively so
/// `PATH` set in an rc file (nvm, volta) is included; it gets no stdin, its own
/// process group, and a short deadline, so an rc file that waits for input or
/// starts a long-lived program cannot hang the request.
#[cfg(unix)]
async fn login_shell_path() -> Option<OsString> {
    probe_path(&login_shell()?, PROBE_TIMEOUT).await
}

#[cfg(unix)]
pub(crate) async fn probe_path(shell: &Path, timeout: Duration) -> Option<OsString> {
    let mut command = tokio::process::Command::new(shell);
    command
        .args(["-l", "-i", "-c", PROBE_SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0);
    if let Some(home) = home_dir() {
        command.current_dir(home);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let mut text = Vec::new();
    let read = async {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) if text.len() < 64 * 1024 => text.extend_from_slice(&chunk[..read]),
                Ok(_) => {}
            }
            if text_has_end_marker(&text) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(timeout, read).await;
    kill_tree(&mut child).await;
    let _ = child.wait().await;
    parse_probe_output(&String::from_utf8_lossy(&text)).map(OsString::from)
}

#[cfg(unix)]
const PROBE_BEGIN: &str = "__KOUSHI_PATH_BEGIN__";
#[cfg(unix)]
const PROBE_END: &str = "__KOUSHI_PATH_END__";
/// `printenv` prints the exported, colon-separated value in every shell,
/// including fish, whose `$PATH` expands to a space-separated list.
#[cfg(unix)]
const PROBE_SCRIPT: &str =
    "printf '%s' '__KOUSHI_PATH_BEGIN__'; printenv PATH; printf '%s' '__KOUSHI_PATH_END__'";

#[cfg(unix)]
fn text_has_end_marker(text: &[u8]) -> bool {
    text.windows(PROBE_END.len())
        .any(|window| window == PROBE_END.as_bytes())
}

/// The `PATH` between the probe's markers, ignoring whatever the rc files
/// printed around it.
#[cfg(unix)]
pub(crate) fn parse_probe_output(text: &str) -> Option<String> {
    let start = text.rfind(PROBE_BEGIN)? + PROBE_BEGIN.len();
    let end = start + text[start..].find(PROBE_END)?;
    let path = text[start..end].trim();
    (!path.is_empty()).then(|| path.to_owned())
}

#[cfg(unix)]
fn login_shell() -> Option<PathBuf> {
    if let Some(shell) = std::env::var_os("SHELL").map(PathBuf::from)
        && shell.is_absolute()
        && shell.is_file()
    {
        return Some(shell);
    }
    // GUI apps do not always get $SHELL; ask the passwd entry.
    // SAFETY: getpwuid returns a pointer into static storage that stays valid
    // until the next getpw* call on this thread; it is copied out at once.
    let from_passwd = unsafe {
        let entry = libc::getpwuid(libc::getuid());
        if entry.is_null() || (*entry).pw_shell.is_null() {
            None
        } else {
            Some(
                std::ffi::CStr::from_ptr((*entry).pw_shell)
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    };
    from_passwd
        .map(PathBuf::from)
        .filter(|shell| shell.is_absolute() && shell.is_file())
        .or_else(|| Some(PathBuf::from("/bin/sh")))
}

/// The system and user `Path` from the registry, so a CLI installed after
/// Koushi started (which updates the registry, not running processes) is
/// found.
#[cfg(windows)]
async fn windows_registry_path() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for key in [
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        r"HKCU\Environment",
    ] {
        let output = tokio::process::Command::new(system32("reg.exe"))
            .args(["query", key, "/v", "Path"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await;
        if let Ok(output) = output
            && let Some(value) = parse_reg_query(&String::from_utf8_lossy(&output.stdout))
        {
            dirs.extend(std::env::split_paths(&expand_environment(&value)));
        }
    }
    dirs
}

/// The value of a `reg query ... /v Path` answer.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn parse_reg_query(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("Path")?.trim_start();
        let value = rest
            .strip_prefix("REG_EXPAND_SZ")
            .or_else(|| rest.strip_prefix("REG_SZ"))?;
        Some(value.trim().to_owned())
    })
}

/// Expands `%NAME%` from the current environment; unknown names stay as they
/// are.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn expand_environment(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(value) if !name.is_empty() => out.push_str(&value),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The client's output as plain text: ANSI escape sequences and control
/// characters removed (line breaks and tabs kept), cut on a character
/// boundary.
pub(crate) fn clean_output(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\u{1b}' => match chars.next() {
                // CSI: parameters, then one final byte in 0x40..=0x7e.
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&next) {
                            break;
                        }
                    }
                }
                // OSC: up to BEL or ESC \.
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push('\n');
                }
            }
            '\n' | '\t' => out.push(character),
            other if other.is_control() => {}
            other => out.push(other),
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
#[path = "register_tests.rs"]
mod tests;
