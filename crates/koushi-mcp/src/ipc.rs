//! Local socket client used by the launcher.
//!
//! One connection per client session. The launcher re-reads `endpoint.json` on
//! every reconnect, so a Koushi restart with a fresh token needs no client
//! restart: the next call reconnects and presents the new token.

use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::endpoint;
use crate::wire::{
    ClientInfo, ClientMessage, EndpointFile, ErrorCode, MAX_FRAME_BYTES, PROTOCOL_VERSION,
    ServerMessage, WireError, decode_line, encode_line,
};

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(unix)]
type ReadHalf = tokio::net::unix::OwnedReadHalf;
#[cfg(unix)]
type WriteHalf = tokio::net::unix::OwnedWriteHalf;

#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(windows)]
type ReadHalf = tokio::net::windows::named_pipe::OwnedReadHalf;
#[cfg(windows)]
type WriteHalf = tokio::net::windows::named_pipe::OwnedWriteHalf;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Covers the app's 50 s maximum long poll plus settlement overhead.
const CALL_TIMEOUT: Duration = Duration::from_secs(70);

/// A live IPC connection. Owned by the single stdio loop, so never shared.
pub struct Client {
    reader: BufReader<ReadHalf>,
    writer: WriteHalf,
    next_id: u64,
}

async fn open(endpoint_path: &EndpointFile) -> Result<Stream, WireError> {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(&endpoint_path.endpoint)
            .await
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new()
            .open(&endpoint_path.endpoint)
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))
    }
}

/// Connects and completes the `hello` handshake.
pub async fn connect(path: &Path, client: ClientInfo) -> Result<Client, WireError> {
    let endpoint_file = endpoint::read(path)
        .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;

    let stream = tokio::time::timeout(CONNECT_TIMEOUT, open(&endpoint_file))
        .await
        .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))??;

    let (read_half, writer) = stream.into_split();
    let mut client_socket = Client {
        reader: BufReader::new(read_half),
        writer,
        next_id: 1,
    };

    let hello = ClientMessage::Hello {
        protocol: PROTOCOL_VERSION,
        token: endpoint_file.token,
        client,
    };
    let line =
        encode_line(&hello).map_err(|_| WireError::new_without_detail(ErrorCode::Internal))?;
    client_socket
        .writer
        .write_all(line.as_bytes())
        .await
        .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;
    client_socket
        .writer
        .flush()
        .await
        .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;

    match client_socket.read_message().await? {
        ServerMessage::HelloOk { protocol, .. } if protocol == PROTOCOL_VERSION => {}
        ServerMessage::HelloOk { .. } => {
            return Err(WireError::new(
                ErrorCode::Unsupported,
                "the running Koushi speaks an unsupported bridge protocol",
            ));
        }
        ServerMessage::Error { code, message } => return Err(WireError::new(code, message)),
        other => {
            return Err(WireError::new(
                ErrorCode::Internal,
                format!("unexpected handshake reply: {other:?}"),
            ));
        }
    }

    Ok(client_socket)
}

impl Client {
    async fn read_message(&mut self) -> Result<ServerMessage, WireError> {
        let mut line = String::new();
        let read = tokio::time::timeout(CALL_TIMEOUT, self.reader.read_line(&mut line))
            .await
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;
        if read == 0 {
            return Err(WireError::new_without_detail(ErrorCode::KoushiUnavailable));
        }
        if line.len() > MAX_FRAME_BYTES {
            return Err(WireError::new_without_detail(ErrorCode::Internal));
        }
        decode_line(&line).map_err(|_| WireError::new_without_detail(ErrorCode::Internal))
    }

    /// Sends one request and waits for its response.
    pub async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, WireError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let line = encode_line(&ClientMessage::Request {
            id,
            method: method.to_owned(),
            params,
        })
        .map_err(|_| WireError::new_without_detail(ErrorCode::Internal))?;
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;
        self.writer
            .flush()
            .await
            .map_err(|_| WireError::new_without_detail(ErrorCode::KoushiUnavailable))?;

        match self.read_message().await? {
            ServerMessage::Response {
                id: got,
                ok,
                result,
                error,
            } if got == id => {
                if ok {
                    Ok(result.unwrap_or(serde_json::Value::Null))
                } else {
                    Err(error.unwrap_or_else(|| WireError::new_without_detail(ErrorCode::Internal)))
                }
            }
            // One request is in flight at a time, so a mismatched id is a
            // protocol violation rather than something to resynchronize.
            ServerMessage::Response { .. } => {
                Err(WireError::new_without_detail(ErrorCode::Internal))
            }
            ServerMessage::Error { code, message } => Err(WireError::new(code, message)),
            ServerMessage::HelloOk { .. } => {
                Err(WireError::new_without_detail(ErrorCode::Internal))
            }
        }
    }
}
