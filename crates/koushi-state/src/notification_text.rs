//! Localized text for notifications that Rust renders outside the WebView
//! (#1476): OS notification banners and remote delivery digests (mail,
//! ServerChan).
//!
//! The WebView message catalog cannot reach these surfaces: the text is built
//! in Rust and shown by the OS or a mail client. This table is their catalog.
//! Every entry has a stable id (the [`NotificationText`] variant), a template
//! for every [`CatalogLocale`], and a per-locale plural rule, and the caller
//! resolves the locale with [`crate::resolve_catalog_locale`] when it renders,
//! so a locale change applies to the next render. Pseudo renders as `en`, as
//! dates and numbers do (`docs/architecture/i18n.md`).
//!
//! Interpolated values are caller data (room labels, partner names, counts) and
//! are inserted verbatim. Localization changes wording only: it never adds or
//! removes message text.

use crate::CatalogLocale;

/// One notification phrase and its values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationText<'a> {
    /// A mention in a one-to-one direct message, named by the partner.
    MentionFrom { name: &'a str },
    /// A one-to-one direct message, named by the partner.
    DirectMessageFrom { name: &'a str },
    /// A mention in a room.
    MentionIn { place: &'a str },
    /// A direct message conversation named by its room label.
    DirectMessageIn { place: &'a str },
    /// A message in a room.
    MessageIn { place: &'a str },
    /// The label of a room that has no usable name.
    RoomFallback,
    /// A digest subject: the first room's headline and how many more rooms.
    SubjectMoreRooms { headline: &'a str, count: u64 },
    /// The tail of a digest body naming how many rooms it does not list.
    BodyMoreRooms { count: u64 },
    /// A mention count.
    MentionCount { count: u64 },
    /// An unread message count.
    UnreadCount { count: u64 },
    /// The subject of a delivery test send.
    TestDeliverySubject,
    /// The body of a delivery test send.
    TestDeliveryBody,
}

/// The languages this table carries. Pseudo renders as English.
#[derive(Clone, Copy)]
enum Lang {
    En,
    Ja,
    ZhHans,
    ZhHant,
    Ko,
}

fn lang(locale: CatalogLocale) -> Lang {
    match locale {
        CatalogLocale::En | CatalogLocale::Pseudo => Lang::En,
        CatalogLocale::Ja => Lang::Ja,
        CatalogLocale::ZhHans => Lang::ZhHans,
        CatalogLocale::ZhHant => Lang::ZhHant,
        CatalogLocale::Ko => Lang::Ko,
    }
}

/// Plural categories the table distinguishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationPlural {
    One,
    Other,
}

/// The plural category of `count` in `locale`: English distinguishes one;
/// Japanese, Chinese, and Korean use one form for every count.
pub fn notification_plural(locale: CatalogLocale, count: u64) -> NotificationPlural {
    match lang(locale) {
        Lang::En if count == 1 => NotificationPlural::One,
        _ => NotificationPlural::Other,
    }
}

/// Joins the parts of a count list, such as "2 mentions, 3 unread".
pub fn notification_list_separator(locale: CatalogLocale) -> &'static str {
    match lang(locale) {
        Lang::En | Lang::Ko => ", ",
        Lang::Ja => "、",
        Lang::ZhHans | Lang::ZhHant => "，",
    }
}

/// Separates a headline from its counts, such as "Message in Room: 2 unread".
pub fn notification_label_separator(locale: CatalogLocale) -> &'static str {
    match lang(locale) {
        Lang::En | Lang::Ja | Lang::Ko => ": ",
        Lang::ZhHans | Lang::ZhHant => "：",
    }
}

/// The phrase in `locale`.
pub fn notification_text(locale: CatalogLocale, text: NotificationText<'_>) -> String {
    use NotificationText as T;
    let one = |count: u64| notification_plural(locale, count) == NotificationPlural::One;
    match (lang(locale), text) {
        (Lang::En, T::MentionFrom { name }) => format!("Mention from {name}"),
        (Lang::Ja, T::MentionFrom { name }) => format!("{name} からのメンション"),
        (Lang::ZhHans, T::MentionFrom { name }) => format!("来自 {name} 的提及"),
        (Lang::ZhHant, T::MentionFrom { name }) => format!("來自 {name} 的提及"),
        (Lang::Ko, T::MentionFrom { name }) => format!("{name}님의 멘션"),

        (Lang::En, T::DirectMessageFrom { name }) => format!("Direct message from {name}"),
        (Lang::Ja, T::DirectMessageFrom { name }) => {
            format!("{name} からのダイレクトメッセージ")
        }
        (Lang::ZhHans, T::DirectMessageFrom { name }) => format!("来自 {name} 的私信"),
        (Lang::ZhHant, T::DirectMessageFrom { name }) => format!("來自 {name} 的私訊"),
        (Lang::Ko, T::DirectMessageFrom { name }) => format!("{name}님의 다이렉트 메시지"),

        (Lang::En, T::MentionIn { place }) => format!("Mention in {place}"),
        (Lang::Ja, T::MentionIn { place }) => format!("{place} でのメンション"),
        (Lang::ZhHans, T::MentionIn { place }) => format!("{place} 中的提及"),
        (Lang::ZhHant, T::MentionIn { place }) => format!("{place} 中的提及"),
        (Lang::Ko, T::MentionIn { place }) => format!("{place}의 멘션"),

        (Lang::En, T::DirectMessageIn { place }) => format!("Direct message in {place}"),
        (Lang::Ja, T::DirectMessageIn { place }) => format!("{place} のダイレクトメッセージ"),
        (Lang::ZhHans, T::DirectMessageIn { place }) => format!("{place} 中的私信"),
        (Lang::ZhHant, T::DirectMessageIn { place }) => format!("{place} 中的私訊"),
        (Lang::Ko, T::DirectMessageIn { place }) => format!("{place}의 다이렉트 메시지"),

        (Lang::En, T::MessageIn { place }) => format!("Message in {place}"),
        (Lang::Ja, T::MessageIn { place }) => format!("{place} のメッセージ"),
        (Lang::ZhHans, T::MessageIn { place }) => format!("{place} 中的消息"),
        (Lang::ZhHant, T::MessageIn { place }) => format!("{place} 中的訊息"),
        (Lang::Ko, T::MessageIn { place }) => format!("{place}의 메시지"),

        (Lang::En, T::RoomFallback) => "Room".to_owned(),
        (Lang::Ja, T::RoomFallback) => "ルーム".to_owned(),
        (Lang::ZhHans, T::RoomFallback) => "房间".to_owned(),
        (Lang::ZhHant, T::RoomFallback) => "聊天室".to_owned(),
        (Lang::Ko, T::RoomFallback) => "방".to_owned(),

        (Lang::En, T::SubjectMoreRooms { headline, count }) if one(count) => {
            format!("{headline} and {count} more room")
        }
        (Lang::En, T::SubjectMoreRooms { headline, count }) => {
            format!("{headline} and {count} more rooms")
        }
        (Lang::Ja, T::SubjectMoreRooms { headline, count }) => {
            format!("{headline} ほか {count} 件のルーム")
        }
        (Lang::ZhHans, T::SubjectMoreRooms { headline, count }) => {
            format!("{headline} 及另外 {count} 个房间")
        }
        (Lang::ZhHant, T::SubjectMoreRooms { headline, count }) => {
            format!("{headline} 及另外 {count} 個聊天室")
        }
        (Lang::Ko, T::SubjectMoreRooms { headline, count }) => {
            format!("{headline} 외 방 {count}개")
        }

        (Lang::En, T::BodyMoreRooms { count }) => format!("and {count} more"),
        (Lang::Ja, T::BodyMoreRooms { count }) => format!("ほか {count} 件"),
        (Lang::ZhHans, T::BodyMoreRooms { count }) => format!("及另外 {count} 个"),
        (Lang::ZhHant, T::BodyMoreRooms { count }) => format!("及另外 {count} 個"),
        (Lang::Ko, T::BodyMoreRooms { count }) => format!("외 {count}개"),

        (Lang::En, T::MentionCount { count }) if one(count) => format!("{count} mention"),
        (Lang::En, T::MentionCount { count }) => format!("{count} mentions"),
        (Lang::Ja, T::MentionCount { count }) => format!("メンション {count} 件"),
        (Lang::ZhHans, T::MentionCount { count }) => format!("{count} 条提及"),
        (Lang::ZhHant, T::MentionCount { count }) => format!("{count} 則提及"),
        (Lang::Ko, T::MentionCount { count }) => format!("멘션 {count}개"),

        (Lang::En, T::UnreadCount { count }) => format!("{count} unread"),
        (Lang::Ja, T::UnreadCount { count }) => format!("未読 {count} 件"),
        (Lang::ZhHans, T::UnreadCount { count }) => format!("{count} 条未读"),
        (Lang::ZhHant, T::UnreadCount { count }) => format!("{count} 則未讀"),
        (Lang::Ko, T::UnreadCount { count }) => format!("읽지 않음 {count}개"),

        (Lang::En, T::TestDeliverySubject) => "Koushi test notification".to_owned(),
        (Lang::Ja, T::TestDeliverySubject) => "Koushi のテスト通知".to_owned(),
        (Lang::ZhHans, T::TestDeliverySubject) => "Koushi 测试通知".to_owned(),
        (Lang::ZhHant, T::TestDeliverySubject) => "Koushi 測試通知".to_owned(),
        (Lang::Ko, T::TestDeliverySubject) => "Koushi 테스트 알림".to_owned(),

        (Lang::En, T::TestDeliveryBody) => "This is a test notification from Koushi.\n\
             If you can read it, this device can reach this delivery sink. No room, message, \
             or account data is included in a test send."
            .to_owned(),
        (Lang::Ja, T::TestDeliveryBody) => "これは Koushi からのテスト通知です。\n\
             これが読めれば、この端末からこの通知先に届けられます。テスト送信にはルーム、\
             メッセージ、アカウントの情報は含まれません。"
            .to_owned(),
        (Lang::ZhHans, T::TestDeliveryBody) => "这是来自 Koushi 的测试通知。\n\
             如果你能看到它，说明此设备可以送达这个通知目标。测试发送不包含任何房间、消息或账户数据。"
            .to_owned(),
        (Lang::ZhHant, T::TestDeliveryBody) => "這是來自 Koushi 的測試通知。\n\
             如果你能看到它，表示此裝置可以送達這個通知目標。測試傳送不包含任何聊天室、訊息或帳號資料。"
            .to_owned(),
        (Lang::Ko, T::TestDeliveryBody) => "Koushi에서 보낸 테스트 알림입니다.\n\
             이 알림이 보이면 이 기기에서 이 알림 대상으로 전달할 수 있습니다. 테스트 전송에는 방, \
             메시지, 계정 정보가 포함되지 않습니다."
            .to_owned(),
    }
}

/// A count list such as "2 mentions, 3 unread", with zero counts left out.
pub fn notification_counts(locale: CatalogLocale, mentions: u64, unread: u64) -> String {
    let mut parts = Vec::new();
    if mentions > 0 {
        parts.push(notification_text(
            locale,
            NotificationText::MentionCount { count: mentions },
        ));
    }
    if unread > 0 {
        parts.push(notification_text(
            locale,
            NotificationText::UnreadCount { count: unread },
        ));
    }
    parts.join(notification_list_separator(locale))
}
