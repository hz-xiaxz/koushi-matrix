//! Submitting one notification through a mailbox's own SMTP relay.
//!
//! This is the sink for the cheap end of the market: a mailbox the user already owns,
//! reached with the authorization code or app password the provider issues for
//! third-party clients (see [`crate::presets`]). It speaks the submission half of
//! RFC 5321 by hand over a rustls connection that trusts the platform store, because the
//! tree has no shared outbound client and this sink must not become the place that
//! invents TLS, proxy, or certificate policy.
//!
//! What the product can rely on: the conversation runs start to finish inside one
//! bounded attempt with no retry and no delay of its own; nothing that is not accepted
//! by the relay is reported as delivered; and the credential is written to the wire
//! once, is never stored on the sink, and never appears in an outcome, a `Debug`, or an
//! error string.

use std::{fmt, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use koushi_key::NotificationSinkSecret;
use rustls::{ClientConfig, crypto::aws_lc_rs, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
    resend::usable_sender_name,
};

/// How long one delivery may take, from the first byte of the TCP connect to the last
/// byte of the `QUIT` reply.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// Longest single reply line this sink will read before it treats the relay as broken.
const REPLY_LINE_MAX_BYTES: u64 = 4096;

/// Most lines one multiline reply may occupy before the relay is treated as broken.
const REPLY_LINES_MAX: usize = 128;

/// How an SMTP connection is protected.
///
/// The choice is the mailbox provider's, not this crate's: a provider that accepts both
/// documents one of them for third-party clients, and the preset carries that one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmtpTls {
    /// TLS from the first byte, as on port 465.
    Implicit,
    /// Plain TCP first, upgraded with `STARTTLS` before the credential is written.
    StartTls,
}

/// Where and how to reach one mailbox's submission relay.
///
/// These are the mailbox's own coordinates, so they are the user's private data: they
/// belong in the device-local settings the user owns, and `username` is the address the
/// relay authenticates. The secret that proves the mailbox is deliberately *not* here;
/// it is a separate credential and reaches the sink per call. Printing these settings
/// shows the relay but never the address.
#[derive(Clone, Eq, PartialEq)]
pub struct SmtpSettings {
    /// Relay host name, as the provider documents it.
    pub host: String,
    /// Relay port, as the provider documents it for this TLS mode.
    pub port: u16,
    /// How the connection is protected.
    pub tls: SmtpTls,
    /// The mailbox address the relay authenticates, not a display name.
    pub username: String,
}

impl fmt::Debug for SmtpSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The mailbox address identifies a real account, and no diagnostic needs it: a
        // support log says which relay and which port, and asks the user for the rest.
        formatter
            .debug_struct("SmtpSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("username", &"<redacted>")
            .finish()
    }
}

/// One mailbox's SMTP relay, as a notification sink.
#[derive(Clone)]
pub struct SmtpSink {
    settings: SmtpSettings,
}

impl SmtpSink {
    /// Build a sink for one relay.
    ///
    /// The settings are the mailbox's own coordinates; the credential that proves the
    /// mailbox is a separate value that arrives per call, so this sink never holds,
    /// caches, or persists one.
    pub fn new(settings: SmtpSettings) -> Self {
        Self { settings }
    }

    /// Deliver one notification through this relay.
    ///
    /// Exactly one attempt is made, inside one bounded window: no retry, no delay, no
    /// second connection, and no envelope beyond the request's own addresses, so a
    /// relay that goes quiet cannot pin the delivery loop. Only the relay's own verdict
    /// leaves, as a typed failure or as a delivery.
    pub async fn send(
        &self,
        secret: &NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> NotificationDeliveryOutcome {
        match tokio::time::timeout(DELIVERY_TIMEOUT, self.deliver(secret, request)).await {
            Ok(Ok(())) => NotificationDeliveryOutcome::Delivered,
            Ok(Err(failure)) => NotificationDeliveryOutcome::Failed(failure),
            // A relay that never answers, or a handshake that never finishes, is a
            // transport failure: no verdict was reached, and the caller owns any retry.
            Err(_) => NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport),
        }
    }

    async fn deliver(
        &self,
        secret: &NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> Result<(), NotificationDeliveryFailure> {
        let settings = &self.settings;
        let stream = TcpStream::connect((settings.host.as_str(), settings.port))
            .await
            .map_err(|_| NotificationDeliveryFailure::Transport)?;
        match settings.tls {
            SmtpTls::Implicit => {
                let stream = tls_connect(stream, &settings.host).await?;
                converse(stream, settings, secret, request).await
            }
            SmtpTls::StartTls => {
                // Greet and introduce ourselves in the clear, then upgrade before a
                // single byte of the credential is written. RFC 3207: the client
                // re-issues `EHLO` on the protected connection and the relay does not
                // greet again. A relay that refuses `STARTTLS` ends the attempt here;
                // falling back to the plaintext connection would hand the credential to
                // anyone who stripped the option.
                let mut session = Session::new(stream);
                session.read_greeting().await?;
                session.ehlo(&settings.host).await?;
                session.start_tls().await?;
                let stream = tls_connect(session.into_inner(), &settings.host).await?;
                let mut session = Session::new(stream);
                session.submit(settings, secret, request).await
            }
        }
    }
}

impl fmt::Debug for SmtpSink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The host and the mailbox address are the user's own private data, and which
        // relay this device talks to is not a diagnostic this crate owes anyone.
        formatter.write_str("SmtpSink(..)")
    }
}

/// Run one complete submission over a stream that is already protected.
///
/// The greeting is read here because a fresh connection starts with one, whether the
/// protection came from implicit TLS or from a `STARTTLS` upgrade the caller performed.
pub(crate) async fn converse<S>(
    stream: S,
    settings: &SmtpSettings,
    secret: &NotificationSinkSecret,
    request: &NotificationDeliveryRequest,
) -> Result<(), NotificationDeliveryFailure>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = Session::new(stream);
    session.read_greeting().await?;
    session.submit(settings, secret, request).await
}

/// Wrap an open TCP connection in TLS, verified against the platform trust store.
///
/// This is the only place this crate decides whether a certificate is trustworthy, and
/// it adds no policy of its own: the platform verifier makes the same judgement the rest
/// of the machine makes. Every way it can fail — an unresolvable host, an untrusted or
/// expired chain, a hostname mismatch, a handshake that never finishes — becomes the one
/// transport failure, because none of them is a verdict about the message.
async fn tls_connect(
    stream: TcpStream,
    host: &str,
) -> Result<TlsStream<TcpStream>, NotificationDeliveryFailure> {
    let server_name = ServerName::try_from(host.to_owned())
        .map_err(|_| NotificationDeliveryFailure::Transport)?;
    TlsConnector::from(Arc::new(client_config()?))
        .connect(server_name, stream)
        .await
        .map_err(|_| NotificationDeliveryFailure::Transport)
}

/// The TLS configuration this sink uses: rustls's own defaults and the OS trust store.
///
/// The crypto provider is named rather than left to rustls's auto-detection on purpose.
/// rustls resolves a process default from its own crate features only while exactly one
/// provider is enabled, and this workspace can enable two — `aws-lc-rs` through reqwest,
/// `ring` through another consumer — in which case the auto-detection panics. A
/// notification sink may not panic, and must not install a process-wide default either,
/// so it names the provider this workspace already compiles in.
fn client_config() -> Result<ClientConfig, NotificationDeliveryFailure> {
    let config = ClientConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| NotificationDeliveryFailure::Transport)?
        .with_platform_verifier()
        .map_err(|_| NotificationDeliveryFailure::Transport)?
        .with_no_client_auth();
    Ok(config)
}

/// One SMTP connection, framed as CRLF-terminated commands and replies.
///
/// The conversation is generic over its stream on purpose: the parts that decide an
/// outcome run over an in-memory pair in tests, so the envelope, the dot-stuffing, the
/// credential encoding, and the reply mapping are proven without a socket, a
/// certificate, or a relay.
pub(crate) struct Session<S> {
    reader: BufReader<S>,
}

impl<S> Session<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    pub(crate) fn new(stream: S) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }

    /// The stream back, for the `STARTTLS` handover.
    ///
    /// Anything the reader buffered past the reply is dropped, and RFC 3207 leaves
    /// nothing to drop: a relay must not send a byte between its `220` and the client's
    /// handshake, so a relay that does is already outside the protocol.
    pub(crate) fn into_inner(self) -> S {
        self.reader.into_inner()
    }

    /// Read the `220` a relay greets a new connection with.
    pub(crate) async fn read_greeting(&mut self) -> Result<(), NotificationDeliveryFailure> {
        let code = self.read_reply().await?;
        if code == 220 {
            Ok(())
        } else {
            Err(Step::Greeting.refusal(code))
        }
    }

    /// Introduce this client. The relay answers a multiline `250` listing what it
    /// supports; nothing in that list is believed, so only the code is read.
    pub(crate) async fn ehlo(&mut self, host: &str) -> Result<(), NotificationDeliveryFailure> {
        self.expect(Step::Ehlo, &format!("EHLO {}", field_value(host)), 250)
            .await
    }

    /// Ask for the upgrade the settings require.
    pub(crate) async fn start_tls(&mut self) -> Result<(), NotificationDeliveryFailure> {
        self.expect(Step::StartTls, "STARTTLS", 220).await
    }

    /// Everything a submission does once its connection is protected: introduce this
    /// client, authenticate, hand over the message, and end the session.
    pub(crate) async fn submit(
        &mut self,
        settings: &SmtpSettings,
        secret: &NotificationSinkSecret,
        request: &NotificationDeliveryRequest,
    ) -> Result<(), NotificationDeliveryFailure> {
        self.ehlo(&settings.host).await?;
        self.authenticate(&settings.username, secret.as_str())
            .await?;
        self.send_message(request).await?;
        self.quit().await;
        Ok(())
    }

    /// `AUTH LOGIN`: the mechanism the mailbox presets document for third-party
    /// clients.
    ///
    /// Both values are base64-encoded immediately before the line that carries them,
    /// written once, and dropped before the next reply is read; neither is stored on the
    /// session, printed, or returned. A refusal here is the only answer the user can act
    /// on, and [`Step::refusal`] maps it accordingly.
    async fn authenticate(
        &mut self,
        username: &str,
        secret: &str,
    ) -> Result<(), NotificationDeliveryFailure> {
        let mut code = self.command("AUTH LOGIN").await?;
        if code != 334 {
            return Err(Step::Auth.refusal(code));
        }
        code = self.command(&STANDARD.encode(username)).await?;
        if code != 334 {
            return Err(Step::Auth.refusal(code));
        }
        code = self.command(&STANDARD.encode(secret)).await?;
        if code != 235 {
            return Err(Step::Auth.refusal(code));
        }
        Ok(())
    }

    /// The envelope and the message: `MAIL FROM`, `RCPT TO`, `DATA`, the body, and the
    /// terminating dot line.
    ///
    /// The envelope is the one place where request text becomes protocol syntax, so both
    /// addresses are reduced to a bare address first. A request that cannot form a valid
    /// envelope is refused here rather than sent as a malformed command, and it is never
    /// reported as delivered.
    async fn send_message(
        &mut self,
        request: &NotificationDeliveryRequest,
    ) -> Result<(), NotificationDeliveryFailure> {
        let from = envelope_address(&request.from_address);
        let to = envelope_address(&request.destination);
        if from.is_empty() || to.is_empty() {
            return Err(NotificationDeliveryFailure::Rejected);
        }
        self.expect(Step::Mail, &format!("MAIL FROM:<{from}>"), 250)
            .await?;
        self.expect(Step::Rcpt, &format!("RCPT TO:<{to}>"), 250)
            .await?;
        self.expect(Step::Data, "DATA", 354).await?;
        self.write_line(&message(request, &from, &to)).await?;
        // One write carries the whole message and the dot line that ends it, so no part
        // of the body can be split, reordered, or followed by a stray command.
        let code = self.read_reply().await?;
        if code == 250 {
            Ok(())
        } else {
            Err(Step::Body.refusal(code))
        }
    }

    /// End the session.
    ///
    /// The relay accepted the message before this point, so a relay that closes the
    /// connection instead of answering belongs to the normal end of a submission and
    /// must never turn a delivered mail into a failure. The reply is read and discarded
    /// on purpose.
    pub(crate) async fn quit(&mut self) {
        let _ = self.command("QUIT").await;
    }

    /// Send one command and read its reply.
    async fn command(&mut self, line: &str) -> Result<u16, NotificationDeliveryFailure> {
        self.write_line(line).await?;
        self.read_reply().await
    }

    /// Send one command and require exactly `expected`.
    async fn expect(
        &mut self,
        step: Step,
        line: &str,
        expected: u16,
    ) -> Result<(), NotificationDeliveryFailure> {
        let code = self.command(line).await?;
        if code == expected {
            Ok(())
        } else {
            Err(step.refusal(code))
        }
    }

    /// Write one CRLF-terminated line.
    async fn write_line(&mut self, line: &str) -> Result<(), NotificationDeliveryFailure> {
        let stream = self.reader.get_mut();
        stream
            .write_all(line.as_bytes())
            .await
            .map_err(|_| NotificationDeliveryFailure::Transport)?;
        stream
            .write_all(b"\r\n")
            .await
            .map_err(|_| NotificationDeliveryFailure::Transport)?;
        stream
            .flush()
            .await
            .map_err(|_| NotificationDeliveryFailure::Transport)?;
        Ok(())
    }

    /// Read one reply, following `250-…` continuation lines to the `250 …` that ends it.
    ///
    /// The whole reply must agree on one code, occupy a bounded number of bounded lines,
    /// and arrive before the connection closes; anything else is a broken conversation
    /// rather than a verdict, and it maps to the transport failure.
    async fn read_reply(&mut self) -> Result<u16, NotificationDeliveryFailure> {
        let mut code = None;
        for _ in 0..REPLY_LINES_MAX {
            let line = self.read_line().await?;
            let (line_code, continues) =
                parse_reply_line(&line).ok_or(NotificationDeliveryFailure::Transport)?;
            match code {
                Some(previous) if previous != line_code => {
                    return Err(NotificationDeliveryFailure::Transport);
                }
                _ => code = Some(line_code),
            }
            if !continues {
                return Ok(line_code);
            }
        }
        Err(NotificationDeliveryFailure::Transport)
    }

    /// Read one LF-terminated line, bounded so a relay cannot make this sink allocate
    /// without limit.
    async fn read_line(&mut self) -> Result<Vec<u8>, NotificationDeliveryFailure> {
        let mut line = Vec::new();
        let mut bounded = (&mut self.reader).take(REPLY_LINE_MAX_BYTES);
        let read = bounded
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| NotificationDeliveryFailure::Transport)?;
        // Zero bytes means the relay closed the connection, and a line without its
        // terminator is longer than any relay may send: neither is a verdict, and neither
        // may be parsed as one.
        if read == 0 || line.last() != Some(&b'\n') {
            return Err(NotificationDeliveryFailure::Transport);
        }
        Ok(line)
    }
}

/// The point of the conversation a reply belongs to.
///
/// The same code means different things at different points, so the mapping is decided
/// deliberately per step rather than guessed from the number alone.
#[derive(Clone, Copy)]
enum Step {
    Greeting,
    Ehlo,
    StartTls,
    Auth,
    Mail,
    Rcpt,
    Data,
    Body,
}

impl Step {
    /// What a refusal at this point means to the product.
    ///
    /// AUTH is read in detail, because the answers there look alike and mean different
    /// things to the user:
    ///
    /// * `535` is the relay saying the mailbox's authorization code or app password is not
    ///   accepted. That is the one answer the user can act on by issuing a new credential,
    ///   so it maps to [`NotificationDeliveryFailure::InvalidSecret`].
    /// * `534` and `530` are the relay refusing the client or the account rather than
    ///   judging the secret — a weaker mechanism than the provider now demands, or
    ///   authentication never completed at all — so they map to
    ///   [`NotificationDeliveryFailure::NotPermitted`].
    /// * Any other 5xx at AUTH (`500`, `502`, `504`: a command or mechanism this relay does
    ///   not implement) says nothing about the credential or the account, and the sink will
    ///   not tell the user their key is wrong when the relay never looked at it. It maps to
    ///   [`NotificationDeliveryFailure::Transport`].
    ///
    /// Outside AUTH the mapping is deliberately blunt, because there the code is always
    /// about the message rather than the credential:
    ///
    /// * A 4xx anywhere, and a 5xx at the envelope or the body, is the relay declining this
    ///   message: an unverified sender, a refused recipient, a message it will not carry.
    ///   The credential is not in question, so these map to
    ///   [`NotificationDeliveryFailure::Rejected`]. `421` (service not available, closing
    ///   channel), `550` (no such recipient) and `454` (temporary authentication failure on
    ///   a relay that requires it) all land here: the relay refused this message on this
    ///   connection, and the sink never retries on its own.
    /// * Anything else — a `2xx` or `3xx` where a final reply was required, or a code the
    ///   protocol does not define for this step — is a relay this build cannot classify,
    ///   so it maps to [`NotificationDeliveryFailure::Transport`] instead of inventing a
    ///   verdict for the product to act on.
    fn refusal(self, code: u16) -> NotificationDeliveryFailure {
        match (self, code / 100) {
            (Self::Auth, 5) => match code {
                535 => NotificationDeliveryFailure::InvalidSecret,
                534 | 530 => NotificationDeliveryFailure::NotPermitted,
                _ => NotificationDeliveryFailure::Transport,
            },
            (_, 4 | 5) => NotificationDeliveryFailure::Rejected,
            _ => NotificationDeliveryFailure::Transport,
        }
    }
}

/// The three-digit code and whether the relay says more lines follow.
///
/// Parsed from bytes, so a relay cannot make this panic with a non-ASCII first three
/// bytes, and a line too short to be a reply is not a reply.
fn parse_reply_line(line: &[u8]) -> Option<(u16, bool)> {
    if line.len() < 3 {
        return None;
    }
    let mut code = 0_u16;
    for byte in &line[..3] {
        if !byte.is_ascii_digit() {
            return None;
        }
        code = code * 10 + u16::from(byte - b'0');
    }
    Some((code, line.get(3) == Some(&b'-')))
}

/// The RFC 5322 message `DATA` carries, ending with the dot line that closes it and no
/// CRLF of its own — [`Session::write_line`] supplies that.
///
/// Header values are reduced to one line each, and every body line is CRLF-terminated
/// with a leading dot doubled, so neither a subject nor a body line can end the command
/// early or add a header of its own.
fn message(request: &NotificationDeliveryRequest, from: &str, to: &str) -> String {
    let mut message = String::new();
    message.push_str("From: ");
    match usable_sender_name(&request.sender_name) {
        Some(sender_name) => {
            message.push_str(&encoded_header_value(sender_name));
            message.push_str(" <");
            message.push_str(from);
            message.push('>');
        }
        // No usable name: the bare address, which is always a well-formed sender.
        None => message.push_str(from),
    }
    message.push_str("\r\nTo: <");
    message.push_str(to);
    message.push_str(">\r\nSubject: ");
    message.push_str(&encoded_header_value(&request.subject));
    // Declared rather than left to be guessed: the body is UTF-8, and a reader must not
    // have to infer an encoding from the bytes.
    message.push_str(
        "\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: 8bit\r\n\r\n",
    );
    for line in body_lines(&request.body) {
        if line.starts_with('.') {
            message.push('.');
        }
        message.push_str(line);
        message.push_str("\r\n");
    }
    // The last line is the terminator, and [`Session::write_line`] adds its CRLF.
    message.push('.');
    message
}

/// The body split into the lines `DATA` must send, with every CRLF or LF ending the
/// request may carry normalized to one entry per line.
fn body_lines(body: &str) -> impl Iterator<Item = &str> {
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// A value that becomes a command or header line, with every control character removed,
/// so request text can never start a new command or a new header.
fn field_value(raw: &str) -> String {
    raw.chars()
        .filter(|character| !character.is_control())
        .collect()
}

/// A header value that may carry non-ASCII text: control characters removed, and anything
/// outside US-ASCII encoded as an RFC 2047 encoded-word.
///
/// A relay is only obliged to accept 8-bit header text when it advertises `SMTPUTF8`
/// (RFC 6531), and this sink does not require that extension, so a display name or a
/// subject written in, say, Japanese is encoded rather than sent as raw bytes that a
/// strict relay may refuse. Only the display name is encoded, never the address beside
/// it: the address has to stay readable for the field to mean anything. An encoded word
/// is base64, whose alphabet cannot contain a control character, so this inherits the
/// single-line guarantee above.
fn encoded_header_value(raw: &str) -> String {
    let value = field_value(raw);
    if value.is_ascii() {
        return value;
    }
    format!("=?UTF-8?B?{}?=", STANDARD.encode(value.as_bytes()))
}

/// The bare address of a `Display Name <address>` form, with everything that could
/// restructure the envelope removed.
///
/// An empty result is not an address, and the caller refuses such a request instead of
/// writing a malformed command.
fn envelope_address(raw: &str) -> String {
    let inner = match (raw.rfind('<'), raw.rfind('>')) {
        (Some(open), Some(close)) if open < close => &raw[open + 1..close],
        _ => raw,
    };
    inner
        .chars()
        .filter(|character| {
            !character.is_control() && !character.is_whitespace() && !matches!(character, '<' | '>')
        })
        .collect()
}
