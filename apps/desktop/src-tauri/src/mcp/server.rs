//! Local socket server hosted by the Tauri adapter.
//!
//! One connection per MCP client session. The launcher authenticates with the
//! per-launch token from `endpoint.json`; this module never reads Matrix
//! credentials and never hands one to a caller.

use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use koushi_mcp::wire::{
    ClientMessage, ErrorCode, MAX_FRAME_BYTES, PROTOCOL_VERSION, ServerMessage, WireError,
    decode_line, encode_line,
};

use super::{endpoint, handlers};

#[cfg(unix)]
type Listener = tokio::net::UnixListener;
#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Listener = NamedPipeListener;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeServer;

/// Owns the listener task and the files it published.
pub(crate) struct BridgeHandle {
    shutdown: watch::Sender<bool>,
    data_dir: PathBuf,
    socket_path: PathBuf,
}

impl BridgeHandle {
    /// Signals the listener to stop and removes the published files.
    pub(crate) fn stop(&self) {
        let _ = self.shutdown.send(true);
        endpoint::remove(&self.data_dir);
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts the bridge: binds the socket, writes `endpoint.json`, and accepts
/// connections until shutdown.
pub(crate) fn start(app: AppHandle, data_dir: PathBuf) -> Result<BridgeHandle, String> {
    endpoint::ensure_dir(&data_dir).map_err(|error| format!("mcp dir: {error}"))?;
    let transport = endpoint::transport_path(&data_dir);
    let token = endpoint::new_token().map_err(|error| format!("mcp token: {error}"))?;
    let socket_path = PathBuf::from(&transport);

    #[cfg(unix)]
    {
        // A stale socket from a crashed run must not block binding.
        let _ = std::fs::remove_file(&socket_path);
        let listener =
            Listener::bind(&socket_path).map_err(|error| format!("mcp bind: {error}"))?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("mcp socket mode: {error}"))?;
        endpoint::write(&data_dir, &transport, &token, env!("CARGO_PKG_VERSION"))
            .map_err(|error| format!("mcp endpoint file: {error}"))?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let log = std::sync::Arc::new(super::events::EventLog::new());
        app.manage(super::events::McpEventLog(std::sync::Arc::clone(&log)));
        super::events::spawn_poller(app.clone(), log, shutdown_rx.clone());
        tauri::async_runtime::spawn(accept_loop(
            listener,
            app,
            token,
            shutdown_rx,
            data_dir.clone(),
        ));
        Ok(BridgeHandle {
            shutdown: shutdown_tx,
            data_dir,
            socket_path,
        })
    }

    #[cfg(windows)]
    {
        let listener = NamedPipeListener::bind(&transport)?;
        endpoint::write(&data_dir, &transport, &token, env!("CARGO_PKG_VERSION"))
            .map_err(|error| format!("mcp endpoint file: {error}"))?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let log = std::sync::Arc::new(super::events::EventLog::new());
        app.manage(super::events::McpEventLog(std::sync::Arc::clone(&log)));
        super::events::spawn_poller(app.clone(), log, shutdown_rx.clone());
        tauri::async_runtime::spawn(accept_loop(
            listener,
            app,
            token,
            shutdown_rx,
            data_dir.clone(),
        ));
        Ok(BridgeHandle {
            shutdown: shutdown_tx,
            data_dir,
            socket_path,
        })
    }
}

#[cfg(unix)]
async fn accept_loop(
    listener: Listener,
    app: AppHandle,
    token: String,
    mut shutdown: watch::Receiver<bool>,
    data_dir: PathBuf,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return;
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let app = app.clone();
                        let token = token.clone();
                        let data_dir = data_dir.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = serve_connection(stream, app, token, data_dir).await;
                        });
                    }
                    Err(_) => {
                        // Transient accept failure; give the runtime a moment
                        // rather than spinning on a broken listener.
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
async fn accept_loop(
    mut listener: NamedPipeListener,
    app: AppHandle,
    token: String,
    mut shutdown: watch::Receiver<bool>,
    data_dir: PathBuf,
) {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    return;
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok(stream) => {
                        let app = app.clone();
                        let token = token.clone();
                        let data_dir = data_dir.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = serve_connection(stream, app, token, data_dir).await;
                        });
                    }
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
}

async fn serve_connection(
    stream: Stream,
    app: AppHandle,
    token: String,
    data_dir: PathBuf,
) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let Some(client) = read_hello(&mut reader, &token).await? else {
        let _ = write_half
            .write_all(
                encode_line(&ServerMessage::Error {
                    code: ErrorCode::Unauthorized,
                    message: "connection token rejected".to_owned(),
                })
                .unwrap_or_default()
                .as_bytes(),
            )
            .await;
        let _ = write_half.flush().await;
        return Ok(());
    };

    let _ = write_half
        .write_all(
            encode_line(&ServerMessage::HelloOk {
                protocol: PROTOCOL_VERSION,
                connection_id: format!("{}-{}", client.name, std::process::id()),
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
            })
            .unwrap_or_default()
            .as_bytes(),
        )
        .await;
    write_half.flush().await?;

    loop {
        let mut line = String::new();
        let read = match reader.read_line(&mut line).await {
            Ok(read) => read,
            Err(_) => return Ok(()),
        };
        if read == 0 {
            return Ok(());
        }
        if line.len() > MAX_FRAME_BYTES {
            let _ = write_half
                .write_all(
                    encode_line(&ServerMessage::Error {
                        code: ErrorCode::Internal,
                        message: "frame too large".to_owned(),
                    })
                    .unwrap_or_default()
                    .as_bytes(),
                )
                .await;
            return Ok(());
        }
        let response = match decode_line::<ClientMessage>(&line) {
            Ok(ClientMessage::Request { id, method, params }) => {
                let result = dispatch_method(&app, &client.name, &data_dir, &method, params).await;
                match result {
                    Ok(value) => ServerMessage::success(id, value),
                    Err(error) => ServerMessage::failure(id, error),
                }
            }
            Ok(ClientMessage::Hello { .. }) => ServerMessage::failure(
                0,
                WireError::new(ErrorCode::Internal, "duplicate handshake"),
            ),
            Err(_) => {
                ServerMessage::failure(0, WireError::new(ErrorCode::Internal, "malformed request"))
            }
        };
        let Ok(encoded) = encode_line(&response) else {
            return Ok(());
        };
        if write_half.write_all(encoded.as_bytes()).await.is_err() {
            return Ok(());
        }
        if write_half.flush().await.is_err() {
            return Ok(());
        }
    }
}

/// Reads and validates the handshake. Returns the declared client on success.
async fn read_hello<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut BufReader<R>,
    expected_token: &str,
) -> std::io::Result<Option<koushi_mcp::wire::ClientInfo>> {
    let mut line = String::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        reader.read_line(&mut line),
    )
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "handshake timeout"))??;
    if read == 0 || line.len() > MAX_FRAME_BYTES {
        return Ok(None);
    }
    let Ok(ClientMessage::Hello {
        protocol,
        token,
        client,
    }) = decode_line::<ClientMessage>(&line)
    else {
        return Ok(None);
    };
    if protocol != PROTOCOL_VERSION {
        return Ok(None);
    }
    if !constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
        return Ok(None);
    }
    Ok(Some(client))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Windows named-pipe listener. Kept small: one instance is prepared before the
/// current one is handed to a connection task.
#[cfg(windows)]
struct NamedPipeListener {
    name: String,
    server: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl NamedPipeListener {
    fn bind(name: &str) -> Result<Self, String> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(name)
            .map_err(|error| format!("mcp pipe: {error}"))?;
        Ok(Self {
            name: name.to_owned(),
            server,
        })
    }

    async fn accept(&mut self) -> std::io::Result<Stream> {
        self.server.connect().await?;
        // Install the next instance before handing this one to a connection
        // task, so a client that connects immediately still finds a pipe.
        let next = Self::bind(&self.name)?;
        let connected = std::mem::replace(&mut self.server, next.server);
        Ok(connected)
    }
}

/// Dispatches one IPC request. Reads are the only tools implemented so far;
/// everything else answers `unsupported` rather than pretending to work.
pub(crate) async fn dispatch_method(
    app: &AppHandle,
    client_name: &str,
    data_dir: &std::path::Path,
    method: &str,
    params: Value,
) -> Result<Value, WireError> {
    match method {
        "tools/call" => handlers::dispatch_tool_call(app, client_name, data_dir, params).await,
        "status" => Ok(serde_json::json!({
            "protocol": PROTOCOL_VERSION,
            "app_version": env!("CARGO_PKG_VERSION")
        })),
        other => Err(WireError::new(
            ErrorCode::Unsupported,
            format!("unsupported bridge method: {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn handshake(input: &str, expected_token: &str) -> Option<koushi_mcp::wire::ClientInfo> {
        let (mut client, server) = tokio::io::duplex(8 * 1024);
        client.write_all(input.as_bytes()).await.expect("write");
        client.shutdown().await.expect("shutdown");
        let mut reader = BufReader::new(server);
        read_hello(&mut reader, expected_token).await.expect("read")
    }

    #[tokio::test]
    async fn accepts_the_matching_token_and_returns_client_info() {
        let input = concat!(
            r#"{"type":"hello","protocol":1,"token":"secret","client":{"name":"codex","version":"0.147.0"}}"#,
            "\n"
        );
        let client = handshake(input, "secret").await.expect("accepted");
        assert_eq!(client.name, "codex");
        assert_eq!(client.version, "0.147.0");
    }

    #[tokio::test]
    async fn rejects_a_wrong_token() {
        let input = concat!(
            r#"{"type":"hello","protocol":1,"token":"guess","client":{"name":"codex","version":"0"}}"#,
            "\n"
        );
        assert!(handshake(input, "secret").await.is_none());
    }

    #[tokio::test]
    async fn rejects_an_unsupported_protocol() {
        let input = concat!(
            r#"{"type":"hello","protocol":9,"token":"secret","client":{"name":"codex","version":"0"}}"#,
            "\n"
        );
        assert!(handshake(input, "secret").await.is_none());
    }

    #[tokio::test]
    async fn rejects_a_non_hello_first_message() {
        let input = r#"{"type":"request","id":1,"method":"tools/call"}"#.to_owned() + "\n";
        assert!(handshake(&input, "secret").await.is_none());
    }

    #[test]
    fn constant_time_comparison_is_length_aware() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
