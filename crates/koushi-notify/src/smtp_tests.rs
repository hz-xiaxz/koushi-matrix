//! The SMTP conversation, driven over an in-memory pair instead of a relay.
//!
//! Nothing here opens a socket, presents a certificate, or reads the OS keychain: the
//! conversation is generic over its stream, so the greeting, the multiline reply parsing,
//! the credential encoding, the envelope, the dot-stuffing, and the reply-to-failure
//! mapping all run against a scripted peer.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};

use crate::presets::{SMTP_PRESETS, preset};
use crate::smtp::{Session, SmtpSettings, SmtpSink, SmtpTls, converse};
use crate::{
    NotificationDeliveryFailure, NotificationDeliveryOutcome, NotificationDeliveryRequest,
};

/// The credential the fake relay is handed. Synthetic, like every value in this file.
const SECRET: &str = "app_password_value";

/// The base64 of the mailbox address above, as `AUTH LOGIN` must put it on the wire.
const USERNAME_BASE64: &str = "c2VuZGVyQGV4YW1wbGUuaW52YWxpZA==";

/// The base64 of [`SECRET`], as the second `AUTH LOGIN` line must carry it.
const SECRET_BASE64: &str = "YXBwX3Bhc3N3b3JkX3ZhbHVl";

fn settings(tls: SmtpTls) -> SmtpSettings {
    SmtpSettings {
        host: "smtp.example.invalid".to_owned(),
        port: 465,
        tls,
        username: "sender@example.invalid".to_owned(),
    }
}

fn secret() -> koushi_key::NotificationSinkSecret {
    koushi_key::NotificationSinkSecret::new(SECRET).expect("secret")
}

fn request() -> NotificationDeliveryRequest {
    NotificationDeliveryRequest {
        subject: "Koushi".to_owned(),
        // One CRLF and one LF ending, and a line that is only `.`: the message must carry
        // the same text with protocol line endings, and that dot must not end `DATA`.
        body: "2 unread in Synthetic Workspace\r\n.\nSent by Koushi".to_owned(),
        destination: "me@example.invalid".to_owned(),
        from_address: "sender@example.invalid".to_owned(),
        sender_name: "Koushi".to_owned(),
        idempotency_key: "delivery-1".to_owned(),
    }
}

/// The relay end of one pair, recording every line the sink sent, CRLF included.
struct FakeRelay {
    stream: BufReader<DuplexStream>,
}

impl FakeRelay {
    fn new(stream: DuplexStream) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    /// Read one line exactly as it arrived.
    async fn next_line(&mut self) -> String {
        let mut line = String::new();
        let read = self.stream.read_line(&mut line).await.expect("relay read");
        assert!(read > 0, "the sink hung up before the script ended");
        assert!(
            line.ends_with("\r\n"),
            "every line the sink writes is CRLF-terminated: {line:?}"
        );
        line
    }

    /// Send raw bytes, so a reply can be multiline.
    async fn say(&mut self, text: &str) {
        self.stream
            .get_mut()
            .write_all(text.as_bytes())
            .await
            .expect("relay write");
        self.stream.get_mut().flush().await.expect("relay flush");
    }

    /// Anything the sink sent after the script, which must be nothing.
    async fn leftover(&mut self) -> Option<String> {
        let mut line = String::new();
        let read = self.stream.read_line(&mut line).await.expect("relay read");
        (read > 0).then_some(line)
    }
}

/// Drive the relay end of one pair through a script and hand back the transcript.
///
/// Each entry is the line the sink must send next, paired with the raw reply the relay
/// gives it (empty for a line that is part of a `DATA` body). Every script ends by
/// requiring the sink to hang up, so a retry, a second connection, or a command out of
/// order fails the test instead of passing unnoticed.
async fn scripted(relay: &mut FakeRelay, greeting: &str, script: &[Turn]) -> Vec<String> {
    relay.say(greeting).await;
    let mut transcript = Vec::new();
    for (_, reply) in script {
        transcript.push(relay.next_line().await);
        if !reply.is_empty() {
            relay.say(reply).await;
        }
    }
    if let Some(leftover) = relay.leftover().await {
        panic!(
            "the sink sent more than the script: {leftover:?}, transcript so far {transcript:?}"
        );
    }
    transcript
}

/// One scripted relay turn: the line the sink must send next, and the raw reply to give
/// it. Owned, so a reply can be written per case.
type Turn = (String, String);

/// One turn.
fn turn(line: &str, reply: &str) -> Turn {
    (line.to_owned(), reply.to_owned())
}

/// A static script in the shape the shared tables are written in.
fn turns(script: &[(&str, &str)]) -> Vec<Turn> {
    script
        .iter()
        .map(|(line, reply)| turn(line, reply))
        .collect()
}

/// Two scripts joined into the one script a relay follows.
fn chain(first: Vec<Turn>, second: Vec<Turn>) -> Vec<Turn> {
    first.into_iter().chain(second).collect()
}

/// The conversation's own result in the shape the product sees.
fn outcome(result: Result<(), NotificationDeliveryFailure>) -> NotificationDeliveryOutcome {
    match result {
        Ok(()) => NotificationDeliveryOutcome::Delivered,
        Err(failure) => NotificationDeliveryOutcome::Failed(failure),
    }
}

/// The lines a script must produce, CRLF included.
fn expected_lines(script: &[Turn]) -> Vec<String> {
    script
        .iter()
        .map(|(line, _)| format!("{line}\r\n"))
        .collect()
}

/// Run one whole conversation against a scripted relay.
async fn run(
    mailbox: SmtpSettings,
    greeting: &'static str,
    script: Vec<Turn>,
) -> (NotificationDeliveryOutcome, Vec<String>) {
    let (client, relay_stream) = duplex(64 * 1024);
    let relay = tokio::spawn(async move {
        let mut relay = FakeRelay::new(relay_stream);
        scripted(&mut relay, greeting, &script).await
    });
    let outcome = outcome(converse(client, &mailbox, &secret(), &request()).await);
    (outcome, relay.await.expect("the relay task"))
}

/// The greeting and first `EHLO` of a relay that advertises more than one capability,
/// which is how real relays answer and what a multiline reply looks like.
const GREETING: &str = "220 relay.example ESMTP\r\n";
const EHLO_WITH_CAPABILITIES: &[(&str, &str)] = &[(
    "EHLO smtp.example.invalid",
    "250-relay.example\r\n250-STARTTLS\r\n250 AUTH LOGIN PLAIN\r\n",
)];

/// Everything a submission does from the credential on, up to the accepted message.
const ACCEPTED_SUBMISSION: &[(&str, &str)] = &[
    ("AUTH LOGIN", "334 VXNlcm5hbWU6\r\n"),
    (USERNAME_BASE64, "334 UGFzc3dvcmQ6\r\n"),
    (SECRET_BASE64, "235 2.7.0 Accepted\r\n"),
    ("MAIL FROM:<sender@example.invalid>", "250 2.1.0 Ok\r\n"),
    ("RCPT TO:<me@example.invalid>", "250 2.1.5 Ok\r\n"),
    ("DATA", "354 End data with <CR><LF>.<CR><LF>\r\n"),
    ("From: Koushi <sender@example.invalid>", ""),
    ("To: <me@example.invalid>", ""),
    ("Subject: Koushi", ""),
    ("MIME-Version: 1.0", ""),
    ("Content-Type: text/plain; charset=utf-8", ""),
    ("Content-Transfer-Encoding: 8bit", ""),
    ("", ""),
    ("2 unread in Synthetic Workspace", ""),
    ("..", ""),
    ("Sent by Koushi", ""),
    (".", "250 2.0.0 Ok: queued\r\n"),
    ("QUIT", "221 2.0.0 Bye\r\n"),
];

#[tokio::test]
async fn an_implicit_tls_submission_sends_exactly_the_decided_message() {
    let script = chain(turns(EHLO_WITH_CAPABILITIES), turns(ACCEPTED_SUBMISSION));
    let expected = expected_lines(&script);

    let (outcome, transcript) = run(settings(SmtpTls::Implicit), GREETING, script).await;

    assert_eq!(outcome, NotificationDeliveryOutcome::Delivered);
    assert!(outcome.is_delivered());
    assert_eq!(outcome.failure(), None);
    // The whole exchange, in order, and nothing after it.
    assert_eq!(transcript, expected);
    // The body line that was only `.` arrives doubled, so it cannot end `DATA` early.
    assert!(transcript.contains(&"..\r\n".to_owned()));
}

#[tokio::test]
async fn starttls_is_negotiated_before_a_single_byte_of_the_credential() {
    let script = chain(
        turns(&[
            EHLO_WITH_CAPABILITIES[0],
            ("STARTTLS", "220 2.0.0 Ready to start TLS\r\n"),
            // RFC 3207: the client re-issues `EHLO` on the protected connection, and the
            // relay does not greet again.
            ("EHLO smtp.example.invalid", "250 relay.example\r\n"),
        ]),
        turns(ACCEPTED_SUBMISSION),
    );
    let (client, relay_stream) = duplex(64 * 1024);
    let relay = tokio::spawn(async move {
        let mut relay = FakeRelay::new(relay_stream);
        scripted(&mut relay, GREETING, &script).await
    });

    let mailbox = settings(SmtpTls::StartTls);
    let mut session = Session::new(client);
    session.read_greeting().await.expect("greeting");
    session.ehlo(&mailbox.host).await.expect("first ehlo");
    session.start_tls().await.expect("starttls");
    // The production sink wraps the stream in TLS here. Everything after the upgrade is
    // the same conversation either way, so keeping the pair tests the ordering without a
    // certificate; the handshake itself is covered only by rustls.
    let mut session = Session::new(session.into_inner());
    session
        .submit(&mailbox, &secret(), &request())
        .await
        .expect("submission");
    // Let the relay see the connection close, so its "nothing more was sent" check can
    // finish.
    drop(session);

    let transcript = relay.await.expect("the relay task");
    assert_eq!(transcript[0], "EHLO smtp.example.invalid\r\n");
    assert_eq!(transcript[1], "STARTTLS\r\n");
    assert_eq!(transcript[2], "EHLO smtp.example.invalid\r\n");
    assert_eq!(transcript[3], "AUTH LOGIN\r\n");
    assert_eq!(transcript[4], format!("{USERNAME_BASE64}\r\n"));
    assert_eq!(transcript[5], format!("{SECRET_BASE64}\r\n"));
    // The credential belongs to the protected connection: nothing before the relay
    // answered 220 to `STARTTLS` may carry a byte of it.
    assert!(
        !transcript[..2]
            .iter()
            .any(|line| line.contains(USERNAME_BASE64) || line.contains(SECRET_BASE64))
    );
}

#[tokio::test]
async fn a_refused_authentication_is_typed_by_which_refusal_the_relay_sent() {
    // The answers at AUTH look alike and mean different things: only 535 is about the
    // credential, 534 and 530 refuse the client or the account, and a relay that does not
    // implement the command never judged the credential at all.
    let cases = [
        (
            "535 5.7.8 Username and Password not accepted",
            NotificationDeliveryFailure::InvalidSecret,
        ),
        (
            "534 5.7.9 Application-specific password required",
            NotificationDeliveryFailure::NotPermitted,
        ),
        (
            "530 5.7.0 Authentication required",
            NotificationDeliveryFailure::NotPermitted,
        ),
        (
            "500 5.5.1 Unrecognized command",
            NotificationDeliveryFailure::Transport,
        ),
        (
            "504 5.7.4 Unrecognized authentication type",
            NotificationDeliveryFailure::Transport,
        ),
        (
            "454 4.7.0 Temporary authentication failure",
            NotificationDeliveryFailure::Rejected,
        ),
    ];

    for (reply, expected) in cases {
        let script = chain(
            turns(&[
                ("EHLO smtp.example.invalid", "250 relay.example\r\n"),
                ("AUTH LOGIN", "334 VXNlcm5hbWU6\r\n"),
            ]),
            vec![turn(USERNAME_BASE64, &format!("{reply}\r\n"))],
        );
        let (outcome, _) = run(settings(SmtpTls::Implicit), GREETING, script).await;

        assert_eq!(
            outcome,
            NotificationDeliveryOutcome::Failed(expected),
            "{reply:?} should map to {expected:?}"
        );
    }
}

#[tokio::test]
async fn a_refused_recipient_is_a_rejection_and_never_a_delivery() {
    let script = turns(&[
        ("EHLO smtp.example.invalid", "250 relay.example\r\n"),
        ("AUTH LOGIN", "334 VXNlcm5hbWU6\r\n"),
        (USERNAME_BASE64, "334 UGFzc3dvcmQ6\r\n"),
        (SECRET_BASE64, "235 2.7.0 Accepted\r\n"),
        ("MAIL FROM:<sender@example.invalid>", "250 2.1.0 Ok\r\n"),
        (
            "RCPT TO:<me@example.invalid>",
            "550 5.1.1 Mailbox unavailable\r\n",
        ),
    ]);
    let (outcome, _) = run(settings(SmtpTls::Implicit), GREETING, script).await;

    assert_eq!(
        outcome,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Rejected)
    );
}

#[tokio::test]
async fn a_relay_that_closes_its_channel_is_a_rejection() {
    // `421` is the relay refusing this connection, not the credential in it.
    let script = turns(&[
        ("EHLO smtp.example.invalid", "250 relay.example\r\n"),
        ("AUTH LOGIN", "334 VXNlcm5hbWU6\r\n"),
        (USERNAME_BASE64, "334 UGFzc3dvcmQ6\r\n"),
        (SECRET_BASE64, "235 2.7.0 Accepted\r\n"),
        ("MAIL FROM:<sender@example.invalid>", "250 2.1.0 Ok\r\n"),
        (
            "RCPT TO:<me@example.invalid>",
            "421 4.3.2 Service not available, closing transmission channel\r\n",
        ),
    ]);
    let (outcome, _) = run(settings(SmtpTls::Implicit), GREETING, script).await;

    assert_eq!(
        outcome,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Rejected)
    );
}

#[tokio::test]
async fn a_relay_that_hangs_up_mid_conversation_is_only_a_transport_failure() {
    let (client, relay_stream) = duplex(64 * 1024);
    let relay = tokio::spawn(async move {
        let mut relay = FakeRelay::new(relay_stream);
        relay.say(GREETING).await;
        let ehlo = relay.next_line().await;
        assert!(
            ehlo.starts_with("EHLO "),
            "unexpected first command: {ehlo:?}"
        );
        // Hang up without answering, which is what a relay that dies mid-conversation
        // looks like from this side.
    });

    let outcome =
        outcome(converse(client, &settings(SmtpTls::Implicit), &secret(), &request()).await);
    relay.await.expect("the relay task");

    assert_eq!(
        outcome,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport)
    );
}

#[tokio::test]
async fn a_relay_that_cannot_be_reached_is_only_a_transport_failure() {
    // Bind and drop a listener so the port is closed: the connect must fail, and the
    // outcome must carry neither the host nor the port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    drop(listener);

    let unreachable = SmtpSettings {
        host: "127.0.0.1".to_owned(),
        port,
        tls: SmtpTls::Implicit,
        username: "sender@example.invalid".to_owned(),
    };
    let outcome = SmtpSink::new(unreachable.clone())
        .send(&secret(), &request())
        .await;

    assert_eq!(
        outcome,
        NotificationDeliveryOutcome::Failed(NotificationDeliveryFailure::Transport)
    );
    assert!(!format!("{outcome:?}").contains(&unreachable.host));
    assert!(!format!("{outcome:?}").contains(&port.to_string()));
}

#[test]
fn the_sink_keeps_its_relay_and_its_mailbox_out_of_debug_output() {
    let sink = SmtpSink::new(settings(SmtpTls::Implicit));

    assert_eq!(format!("{sink:?}"), "SmtpSink(..)");
    assert!(!format!("{sink:?}").contains("smtp.example.invalid"));
    assert!(!format!("{sink:?}").contains("sender@example.invalid"));
}

#[test]
fn the_settings_show_the_relay_and_never_the_mailbox_address() {
    let settings = settings(SmtpTls::StartTls);
    let rendered = format!("{settings:?}");

    // The relay and the port are what a diagnostic needs; the address identifies a real
    // account and is redacted.
    assert!(rendered.contains("smtp.example.invalid"), "{rendered}");
    assert!(rendered.contains("465"), "{rendered}");
    assert!(rendered.contains("StartTls"), "{rendered}");
    assert!(rendered.contains("<redacted>"), "{rendered}");
    assert!(!rendered.contains("sender@example.invalid"), "{rendered}");
}

#[test]
fn the_presets_are_the_three_documented_mailboxes() {
    assert_eq!(SMTP_PRESETS.len(), 3);

    let qq_mail = preset("qq-mail").expect("the QQ Mail preset");
    assert_eq!(qq_mail.host, "smtp.qq.com");
    assert_eq!(qq_mail.port, 465);
    assert_eq!(qq_mail.tls, SmtpTls::Implicit);
    assert!(qq_mail.username_is_full_address);

    let gmail = preset("gmail").expect("the Gmail preset");
    assert_eq!(gmail.host, "smtp.gmail.com");
    assert_eq!(gmail.port, 465);
    assert_eq!(gmail.tls, SmtpTls::Implicit);
    assert!(gmail.username_is_full_address);

    let icloud = preset("icloud").expect("the iCloud preset");
    assert_eq!(icloud.host, "smtp.mail.me.com");
    assert_eq!(icloud.port, 587);
    assert_eq!(icloud.tls, SmtpTls::StartTls);
    assert!(icloud.username_is_full_address);

    // A lookup is exact: a stale or differently-cased id is a miss, never a guess at
    // another provider's relay.
    assert!(preset("QQ-Mail").is_none());
    assert!(preset("qqmail").is_none());
    assert!(preset("").is_none());
}

#[test]
fn every_preset_id_can_key_a_stored_credential() {
    let mut ids: Vec<&str> = Vec::new();
    for entry in SMTP_PRESETS {
        // The id is what the credential store names the secret after, so it passes the
        // same validation the store applies.
        assert!(
            koushi_key::NotificationSinkSecretId::parse(entry.id).is_ok(),
            "preset id {:?} is not a valid credential id",
            entry.id
        );
        assert!(!entry.label.is_empty());
        // The hint is a catalog key the settings layer resolves, not a sentence: text the
        // user reads is product copy and lives in the message catalog.
        assert!(
            entry.secret_hint.starts_with("settings.")
                && !entry.secret_hint.contains(char::is_whitespace),
            "preset {:?} names {:?}, which is not a catalog key",
            entry.id,
            entry.secret_hint
        );
        ids.push(entry.id);
    }
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "preset ids are unique");
}
