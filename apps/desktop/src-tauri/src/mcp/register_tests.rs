use super::*;

#[test]
fn only_absolute_unique_directories_are_searched() {
    let dirs = absolute_unique(vec![
        PathBuf::from("."),
        PathBuf::from(""),
        PathBuf::from("relative/bin"),
        std::env::temp_dir(),
        std::env::temp_dir(),
    ]);
    assert_eq!(dirs, vec![std::env::temp_dir()]);
}

#[test]
fn output_is_plain_text() {
    let raw = b"\x1b[31mError:\x1b[0m koushi \x1b]0;title\x07already exists\r\nnext\x07line\r";
    assert_eq!(clean_output(raw), "Error: koushi already exists\nnextline");
}

#[test]
fn registry_values_are_parsed_and_expanded() {
    let answer = "\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_EXPAND_SZ    %KOUSHI_TEST_UNSET%\\npm;C:\\Tools\r\n\r\n";
    let value = parse_reg_query(answer).expect("value");
    assert_eq!(value, "%KOUSHI_TEST_UNSET%\\npm;C:\\Tools");
    assert_eq!(expand_environment(&value), value, "unknown names stay");
    assert_eq!(expand_environment("100% sure"), "100% sure");
    assert!(
        parse_reg_query("ERROR: The system was unable to find the specified registry key")
            .is_none()
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn the_probe_output_is_read_between_markers() {
        let noisy = "Last login: today\n__KOUSHI_PATH_BEGIN__/opt/node/bin:/usr/bin\n__KOUSHI_PATH_END__bye";
        assert_eq!(
            parse_probe_output(noisy).as_deref(),
            Some("/opt/node/bin:/usr/bin")
        );
        assert_eq!(parse_probe_output("no markers"), None);
    }

    #[test]
    fn programs_resolve_to_the_first_executable_match() {
        let first = tempfile::tempdir().expect("tempdir");
        let second = tempfile::tempdir().expect("tempdir");
        std::fs::write(first.path().join("claude"), "not executable").expect("write");
        let real = script(second.path(), "claude", "exit 0");
        let dirs = vec![first.path().to_path_buf(), second.path().to_path_buf()];
        assert_eq!(resolve_program("claude", &dirs), Some(real));
        assert_eq!(resolve_program("codex", &dirs), None);
    }

    #[tokio::test]
    async fn a_successful_cli_receives_the_exact_arguments() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cli = script(
            dir.path(),
            "claude",
            r#"for a in "$@"; do printf '[%s]' "$a"; done"#,
        );
        let launcher = "/Apps/Kou shi's \"x\"/koushi-mcp";
        let (_, args) = command_argv(RegistrationClient::ClaudeCode, launcher).expect("argv");
        let outcome = run(
            &cli,
            &args,
            &[dir.path().to_path_buf()],
            Duration::from_secs(10),
        )
        .await;
        let expected: String = args.iter().map(|arg| format!("[{arg}]")).collect();
        assert!(
            matches!(outcome, RegisterOutcome::Registered { ref output } if *output == expected),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn a_failing_cli_reports_its_exit_code_and_message() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cli = script(
            dir.path(),
            "codex",
            "echo 'MCP server koushi already exists' >&2; exit 1",
        );
        let outcome = run(&cli, &[], &[], Duration::from_secs(10)).await;
        assert!(
            outcome
                == RegisterOutcome::Failed {
                    exit_code: Some(1),
                    output: "MCP server koushi already exists".to_owned()
                },
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn a_cli_that_asks_for_input_fails_instead_of_waiting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cli = script(dir.path(), "pi", "read answer || exit 3; echo got");
        let started = std::time::Instant::now();
        let outcome = run(&cli, &[], &[], Duration::from_secs(20)).await;
        assert!(
            matches!(
                outcome,
                RegisterOutcome::Failed {
                    exit_code: Some(3),
                    ..
                }
            ),
            "{outcome:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_hanging_cli_and_its_children_are_killed_at_the_deadline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("survived");
        let cli = script(
            dir.path(),
            "claude",
            &format!("(sleep 3; touch '{}') & sleep 30", marker.display()),
        );
        let started = std::time::Instant::now();
        let system = [PathBuf::from("/bin"), PathBuf::from("/usr/bin")];
        let outcome = run(&cli, &[], &system, Duration::from_millis(500)).await;
        assert_eq!(outcome, RegisterOutcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_secs(4)).await;
        assert!(
            !marker.exists(),
            "the background child was killed with the group"
        );
    }

    #[tokio::test]
    async fn a_successful_cli_leaves_nothing_running_and_keeps_its_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let marker = dir.path().join("survived");
        // The CLI succeeds but leaves a child holding stdout open for far
        // longer than the run's output-read deadline. On timeout the readers
        // cap the run at ~4s; a prompt return therefore proves the child was
        // killed with the group, not merely abandoned. The child touches its
        // marker after a second, well before the cleanup check below.
        let cli = script(
            dir.path(),
            "claude",
            &format!(
                "echo 'Added koushi'; (sleep 1; touch '{}'; sleep 30) & exit 0",
                marker.display()
            ),
        );
        let system = [PathBuf::from("/bin"), PathBuf::from("/usr/bin")];
        let started = std::time::Instant::now();
        let outcome = run(&cli, &[], &system, Duration::from_secs(20)).await;
        assert_eq!(
            outcome,
            RegisterOutcome::Registered {
                output: "Added koushi".to_owned()
            }
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        tokio::time::sleep(Duration::from_secs(4)).await;
        assert!(
            !marker.exists(),
            "the leftover child was killed with the group"
        );
    }

    #[tokio::test]
    async fn the_probe_reads_a_noisy_shell_and_survives_a_hanging_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        // The fake shell ignores its flags and runs the probe's script.
        let noisy = script(
            dir.path(),
            "noisy-sh",
            r#"echo 'welcome'; eval "$4"; echo 'goodbye'"#,
        );
        let path = probe_path(&noisy, Duration::from_secs(5))
            .await
            .expect("path");
        assert_eq!(path, std::env::var_os("PATH").unwrap_or_default());

        let hanging = script(dir.path(), "hanging-sh", "sleep 30 & sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(probe_path(&hanging, Duration::from_millis(500)).await, None);
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    #[test]
    fn an_exe_is_preferred_over_a_batch_shim_in_the_same_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("claude.cmd"), "@echo off").expect("write");
        std::fs::write(dir.path().join("claude.exe"), b"MZ").expect("write");
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(
            resolve_program("claude", &dirs),
            Some(dir.path().join("claude.exe"))
        );
    }

    #[tokio::test]
    async fn a_batch_shim_receives_a_launcher_path_with_spaces_as_one_argument() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shim = dir.path().join("claude.cmd");
        // %~7 is the launcher: mcp add --scope user koushi -- <launcher>.
        std::fs::write(&shim, "@echo off\r\necho [%~7]\r\n").expect("write");
        let launcher = r"C:\Program Files\Koushi\koushi-mcp.exe";
        let (_, args) = command_argv(RegistrationClient::ClaudeCode, launcher).expect("argv");
        let system = std::env::var_os("SystemRoot")
            .map(|root| vec![PathBuf::from(root).join("System32")])
            .unwrap_or_default();
        let outcome = run(&shim, &args, &system, Duration::from_secs(20)).await;
        // A path with spaces must reach the shim intact, not be refused.
        match outcome {
            RegisterOutcome::Registered { output } => assert_eq!(output, format!("[{launcher}]")),
            other => panic!("expected registered, got {other:?}"),
        }
    }
}
