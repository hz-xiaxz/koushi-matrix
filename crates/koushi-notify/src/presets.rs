//! The mailboxes this build can offer by name.
//!
//! A preset is a starting point the user can recognize, not a promise: the provider
//! remains the only authority on whether a credential works, and a provider that moves
//! its relay or renames its credential is a data change in this table rather than a
//! change in the sink.

use crate::smtp::SmtpTls;

/// One mailbox a user can pick instead of filling in a relay by hand.
///
/// Every field is what the provider documents for third-party clients at the time this
/// table was written, and each entry names the page it was read from. Nothing here is a
/// secret: the credential itself is a separate value the user enters, and `secret_hint`
/// only says which of the provider's own credentials the relay expects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmtpPreset {
    /// Stable id the settings layer and the credential store key on.
    pub id: &'static str,
    /// The vendor's own proper name, which is not translated: a product name like "Gmail"
    /// reads the same in every locale, and inventing a translated one would make the
    /// account harder to recognize rather than easier.
    pub label: &'static str,
    /// Relay host name.
    pub host: &'static str,
    /// Relay port, for the [`Self::tls`] mode below.
    pub port: u16,
    /// How this provider tells third-party clients to protect the connection.
    pub tls: SmtpTls,
    /// Whether the relay authenticates the full address rather than its local part.
    pub username_is_full_address: bool,
    /// Catalog key for the sentence that tells the user which of the provider's own
    /// credentials to go and fetch.
    ///
    /// The text itself is product copy and lives in the message catalog, so the settings
    /// layer resolves this key there and never renders it directly; the key is stable
    /// because it is derived from [`Self::id`], not from the provider's wording.
    pub secret_hint: &'static str,
}

/// The mailbox presets this build ships.
///
/// Each entry was checked against the provider's own documentation, named in the comment
/// above it, and none of them is a recommendation: a self-hosted relay or a paid
/// transactional provider is still the better choice for volume, and the user's own
/// credentials decide the rest. The order leads with the mailbox a user in this product's
/// first market is most likely to already own.
pub const SMTP_PRESETS: &[SmtpPreset] = &[
    // QQ Mail / Foxmail, Tencent. <https://help.mail.qq.com/detail/108/1089/>: the relay
    // is `smtp.qq.com` on port 465 or 587 with SSL/TLS, 465 first and 587 as the fallback
    // for clients that ask for TLS; the username is the full address and the password is
    // the 16-digit authorization code (授权码) a third-party client must use instead of
    // the account password — see <https://help.mail.qq.com/detail/0/1087>. This is also
    // the mailbox whose own settings can bind a new-mail reminder to WeChat, which is why
    // it leads: a user who wants a nudge on their phone usually already has the mailbox
    // that can give it.
    SmtpPreset {
        id: "qq-mail",
        label: "QQ Mail",
        host: "smtp.qq.com",
        port: 465,
        tls: SmtpTls::Implicit,
        username_is_full_address: true,
        secret_hint: "settings.notificationDeliverySecretHint.qqMail",
    },
    // Gmail. <https://developers.google.com/workspace/gmail/imap/imap-smtp>: the outgoing
    // SMTP server is `smtp.gmail.com`, port 465 for SSL and port 587 for TLS
    // (`STARTTLS`). The credential is a Google app password, which Google only issues
    // while 2-Step Verification is turned on — see
    // <https://support.google.com/accounts/answer/185833>.
    SmtpPreset {
        id: "gmail",
        label: "Gmail",
        host: "smtp.gmail.com",
        port: 465,
        tls: SmtpTls::Implicit,
        username_is_full_address: true,
        secret_hint: "settings.notificationDeliverySecretHint.gmail",
    },
    // iCloud Mail, Apple. <https://support.apple.com/en-us/102525>: the outgoing server is
    // `smtp.mail.me.com` on port 587, authentication is required, the username is the
    // full iCloud Mail address, and the password is an app-specific password. Apple
    // documents that port with SSL and names "TLS or STARTTLS" as the alternative, which
    // is what port 587 means for a submission client.
    SmtpPreset {
        id: "icloud",
        label: "iCloud Mail",
        host: "smtp.mail.me.com",
        port: 587,
        tls: SmtpTls::StartTls,
        username_is_full_address: true,
        secret_hint: "settings.notificationDeliverySecretHint.icloud",
    },
];

/// The preset with this id, or `None` when the id names no preset in this build.
///
/// The id is the same string that keys the mailbox's stored credential, so an id this
/// build no longer ships is a lookup miss rather than a guess at some other provider's
/// relay: a stale id must never silently deliver through whatever now happens to be
/// first.
pub fn preset(id: &str) -> Option<&'static SmtpPreset> {
    SMTP_PRESETS.iter().find(|candidate| candidate.id == id)
}
