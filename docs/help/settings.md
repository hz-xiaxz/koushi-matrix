# Settings and help

[Back to the user guide](README.md)

## Open settings

Select the account icon at the bottom of the left rail to open **Account
Settings** for that account. Select the gear in the top bar to open **App
Settings**, which apply across account tabs. On macOS, the **Koushi** menu has
these as separate **Account Settings…** and **App Settings…** items; **Cmd+,**
opens **App Settings**. Choosing either item while settings are already open
switches directly to that scope. With **Account Settings** open, use the button
at the bottom of the category list to switch to **App Settings**; to return,
open **Account Settings** from the rail or the **Koushi** menu.

Both scopes open in a foreground dialog. Choose a category on the left; its
controls appear on the right. Each side scrolls when the window is small. Close
the dialog with its close button or **Esc**. With focus on a category, use
Up/Down or Home/End to select another category.

Most switches and choices save when selected. Profile and account forms have
their own action buttons. Availability depends on your account, homeserver,
platform, and encryption state; disabled controls are not a promise that the
operation is currently available.

## Where to find each setting

Paths start with **App Settings** or **Account Settings**. Names match the
English UI; Japanese category names are included to help find them in a
translated app.

| Scope and category | Settings and actions |
| --- | --- |
| App Settings → Appearance (外観) | Language (English or 日本語); theme; display density; UI font and emoji style. |
| App Settings → Notifications (通知) | Notification sounds and badge counts, and the device notification controls described under [Notifications from this device](#notifications-from-this-device). Operating-system permissions also apply. |
| App Settings → Preferences (環境設定) | Code-block wrapping; hiding removed messages; close to tray where configurable; automatic loading of older messages; placement of threaded conversations at their latest reply. |
| App Settings → Keyboard (キーボード) | Send-message shortcut (Enter or the platform modifier+Enter); reference list of keyboard shortcuts and their availability. |
| App Settings → Search history (検索履歴) | Shared background-work speed and pause/resume control. Search crawling and media prefetch use this budget across all accounts. **Off** pauses both. |
| App Settings → Help & About (ヘルプと情報) | Public GitHub repository URL, copy button, and instructions for asking ChatGPT or another AI assistant. |
| Account Settings → Account (アカウント) | Profile display name and avatar; account management page when provided by the server; change password and deactivate account when supported. |
| Account Settings → Sessions (セッション) | Homeserver, user ID, device ID/name, verification, cross-signing, backup and local-store information; Sign out. This is current-session information, not a list of all remote devices. The status shown is the last checked result; Koushi re-checks it automatically when the app or account starts and after a connection outage, and the refresh action checks again immediately. |
| Account Settings → Notifications (通知) | **This device** and **Homeserver** tabs. **This device**: desktop notifications, and notifications Koushi sends from this computer through a mailbox or relay you configure. **Homeserver**: account-wide notification rules and **Email notifications**, which apply to every device. See [Notifications from this device](#notifications-from-this-device) and [Email notifications](#email-notifications). |
| Account Settings → Security & Privacy (セキュリティとプライバシー) | URL previews in unencrypted/encrypted rooms; sending read receipts and typing notifications. |
| Account Settings → Encryption (暗号化) | Identity/session verification and trust; secure backup setup and passphrase changes; recovery; encrypted room-key import/export; local-encryption diagnostics and local-data reset. Actions appear according to the current encryption state. |
| Account Settings → Search history (検索履歴) | Indexing of media captions and file names; crawler activity, per-room progress and start/stop actions; rebuild search database. |

Use **Ctrl + -** / **Ctrl + +** (on macOS
**Cmd + -** / **Cmd + +**) to shrink or enlarge the whole interface, and
**Ctrl + 0** (macOS **Cmd + 0**) to reset it. **Ctrl/Cmd + =** also enlarges
the interface on keyboards with an unshifted equals key.
The **View** menu also offers **Zoom In**, **Zoom Out**, and **Actual Size**.
Koushi's native menu labels follow the app language; in Japanese, **View** is **表示**.
On macOS, **Cmd + Ctrl + F** toggles fullscreen. These window shortcuts
remain available while an in-app dialog is open.

The category order follows Element where Koushi has corresponding settings.
Koushi-specific local indexing lives in **Account Settings → Search history**;
its shared cross-account background-work budget is in **App Settings → Search
history**. Koushi does not have all Element settings; consult this table instead
of assuming exact parity. Room-specific settings remain in the room's own menu
and details panel.

On macOS, choose **Koushi → Check for Updates…** to open **Software update**
directly, including before sign-in. This is separate from account settings;
you do not need to navigate through Preferences or Display.
**Automatically check for updates** checks once when the app starts and then
every 24 hours while enabled (enabled by default). Its switch is in the same
Software update dialog. **Include pre-release versions** also considers
SemVer versions such as `1.2.0-alpha.1`, `1.2.0-beta.1`, and `1.2.0-rc.1`.
Use **Check for updates** for an immediate result; when no newer version is
found, the result says that the current version is up to date. An automatic
discovery opens the same screen. Closing the dialog, pressing Escape, or
leaving it unanswered is temporary: the next scheduled or manual check presents
the offer again without reopening immediately. **Ignore this version** stops
automatic reminders for that exact version until Koushi exits; there is no
in-app way to undo it, and signing out or switching accounts does not reset it.
While a version is ignored, the Software update dialog says so and
**Download update** remains available. Background checks continue, a newer
eligible release is still announced, and a manual check still reports the
ignored version. If a later check cannot reach the feed, the existing offer
stays available and the dialog reports the failed check. Downloading and
restarting are separate,
explicit actions; automatic checks do not automatically install or restart.
Unsupported builds show that in-app updates are unavailable. When Koushi was
installed by a distribution package that manages its files (for example an AUR
package), the dialog instead says that updates are provided by your package
manager; update Koushi through that package manager.
Changing the pre-release setting discards an unapproved candidate and checks the
new channel when automatic checks are enabled. Once you choose Download update,
that release stays selected; the pre-release switch is disabled until the
download/install flow ends. Turning automatic checks off does not remove an
already offered release or stop a manually requested check. An undownloaded
offer does not block later checks: when a newer eligible release exists, the
next check replaces the old offer with the newest one.

On Windows, release builds with updater trust material offer separate download
and restart-to-install actions. Downloads are signature-verified before they
become ready. Koushi closes its sessions before launching the installer, which
reopens the updated app. Builds without updater trust material show a
notification and open that specific Release page instead. Windows installers
remain Authenticode-unsigned pending code-signing approval, so Windows may
still show publisher warnings. Linux builds remain unsupported by the in-app updater.

On macOS, dialogs and viewers leave space above their contents for the standard
window buttons. Small windows scroll within the dialog. Escape closes the
topmost dismissible dialog and returns focus to its opener.

For consequences and prerequisites, see [Search](search.md),
[Security and recovery](security-and-recovery.md), and
[User trust model](user-trust-model.md) before changing the corresponding settings.

## Email notifications

**Account Settings → Notifications → Email notifications** has two separate parts:

- The **Email notifications** switch controls delivery. **Off** means your
  homeserver does not email you summaries, even when an address is listed.
  With several addresses, **Send to** chooses the single delivery target.
- **Registered email addresses** lists the email addresses your homeserver has
  registered for your Matrix account. Koushi does not create them. Each shows
  **Email confirmed** (Japanese: メールアドレス確認済み), meaning the homeserver
  records that ownership of the address was confirmed by email; the current
  delivery target shows **Email confirmed · Notification target**. This is
  unrelated to device or contact verification (検証), and a listed address is
  not a guarantee that mail to it is delivered. **Why is this address shown?**
  repeats this explanation.

What you can do with addresses depends on the server:

- When adding is available, **Add email address** (or **Add another email
  address**) sends a confirmation email. Open its link, then press
  **Continue**; a password prompt may appear. The new address is registered
  after confirmation; if email notifications are on, they move to it from
  the current address, which stays registered.
- **This server does not allow adding email addresses here.** means the server
  disallows adding addresses from clients. Addresses already listed stay
  registered, and you can still turn email notifications on or off for them.
- If your account uses an account management page, **Email addresses for this
  account are managed on your account page.** appears with **Manage account &
  devices** when the server provides that page.

## Notifications from this device

**Account Settings → Notifications** is split into two tabs, **This device** and
**Homeserver**. Each tab says who sends the notification: **This device** is
Koushi sending from this computer through a mailbox or relay you configure here,
and **Homeserver** is your own server's settings, which apply to every device on
the account. The two are independent. The **This device** tab is selected first;
the **Homeserver** tab appears only while you are signed in. The device settings
are app-wide, so **App Settings → Notifications** shows the same controls without
the tabs.

Device notifications are **off** by default. Koushi sends nothing from this
device until you turn on **Send notifications from this device** and add at least
one sending service. Koushi sends only while it is running: a notification
waiting for its wait time or for the shortest gap is held in memory only, and
closing Koushi discards it.

Because Koushi sends these itself, the feature works on a homeserver with no
email support. A notification is a summary, not the message: it names the rooms
that need attention, each room's kind, and its unread and mention counts. No
message text leaves this device unless you turn on the plaintext prefix below.

### Choose what is sent

**Send for** has three switches, on by default:

- **Direct messages**
- **Mentions and replies**
- **Your keywords**

A plain group message matches none of them. To be told about one room or a whole
space whatever these switches say, use the watch list below.

**Also send for these rooms and spaces** names rooms and spaces that are always
sent. Choose **Add a room or space** and pick from your room list; **Remove**
drops an entry. A watched room is sent even when its kind is off. A watched space
covers every room in that space, including rooms added to it later, so you do not
have to edit the list when the space grows. A room you read or mute is still
skipped, and the list holds up to 64 rooms and spaces.

### Keywords

**Your keywords** delivers for a keyword hit. A keyword is a Matrix `content`
rule on your account, matched against the message body as a whole word and
without regard to case. A hit arrives as a mention, so either this switch or
**Mentions and replies** sends it. The rules live on your homeserver, not in
Koushi's settings, so a rule another Matrix client set counts here too.

### When a notification is sent

A room is not sent the moment it becomes unread.

- **Wait before sending** (5 minutes by default) is how long a room must stay
  unread before it can be sent. Reading the room, muting it, or, with the focus
  option below on, bringing Koushi to the foreground cancels the wait, and it
  starts again the next time the room becomes unread.
- **Shortest gap between sends** (15 minutes by default) is how long further
  notifications wait after a send, so a busy room cannot fill your mailbox. What
  comes due in the meantime is collected into the next summary.
- **Only while Koushi is not focused** (on by default) holds notifications while
  the Koushi window is in the foreground, the way the operating system suppresses
  a banner you are already looking at.

One summary names a few rooms and then counts the rest, such as "and 3 more". It
never contains a room's internal ID.

### Add a sending service

**Send through** lists the services Koushi delivers through, each turned on or
off with **Send through this service**. Add one with **Add a service**:

1. Choose **Service**: **Resend (email API)** or **SMTP relay**.
2. Fill in **From address** (what the recipient sees as the sender), **Name** (an
   optional display name), and **Send to** (where the notification goes).
3. For **Resend (email API)**, leave **Endpoint** blank to use the endpoint the
   service documents for its email API, unless you have a reason to point it
   elsewhere. For **SMTP relay**, choose a **Mailbox preset** (see
   [Set up a sending service](#set-up-a-sending-service)) or **Other server**,
   then enter **Server**, **Port**, **Connection security**, and **Username**.
4. Choose **Add**. In the service's card, type its password or API key into **App
   password or API key**, then choose **Save**.

The password field is write-only: Koushi never shows the saved value again. It
reports **Saved. Enter a new value to replace it.** or **Not set**. The value is
kept in your operating system's credential store, not in Koushi's settings, and a
diagnostics report never carries it. **Clear** deletes it, and removing the
service deletes it too. You can configure up to eight services.

**Send a test** sends one message through the service you are editing, using the
saved password. A test holds no room, message, sender, or account data; it does
not count toward the shortest gap, and you can send it before the channel or the
service is turned on. **Last send** shows the most recent result for that service,
including a test.

### What the last send result means

- **Delivered**: The service accepted the message. This is the service's own
  answer, not proof that it reached an inbox.
- **The service rejected the key. Enter it again.**: The saved API key, app
  password, or authorization code is wrong, expired, or revoked. Create a new one
  with the provider and save it.
- **The service refused this sender or this account. Check the from address and
  the account status.**: The provider will not send as this **From address**, or
  the account is not allowed to send yet. Some services send only from a verified
  address or domain.
- **The service refused the message.**: The provider rejected this message or
  its recipient, not the credential or the account.
- **The service is limiting how often it accepts messages. Koushi will try again
  later.**: The provider asked Koushi to slow down, so the next attempt waits for
  the provider's own delay or the shortest gap.
- **Could not reach the service.**: A network, TLS, or server problem. Koushi
  retries within the shortest gap.

Koushi retries only a rate limit and a connection problem. A rejected key,
sender, or message is not retried; fix it and the next notification tries again.
A service whose last send failed says **This service is not sending** on its row
while its switch stays on, so an enabled service is never read as a working one.
If nothing arrives at all, see [Notifications from this device do not
arrive](troubleshooting.md#notifications-from-this-device-do-not-arrive).

### Include the start of the message

**Include the start of the message** is off by default. Turning it on opens a
consent you must accept. The consent states that the start of the message leaves
this device as plain text and passes through the service to the destination
without end-to-end encryption; that whether, and for how long, that service keeps
it depends on that service's own terms, which you should read before turning the
option on; and that room names and counts are sent either way. Koushi asserts
nothing about a particular service's retention.

Once you accept, **Characters** sets how many characters of the message start are
added (80 by default, at most 200). The text is reduced to one line and cut at a
character boundary, so it cannot split a character or invent an extra line.

The consent covers one service, one endpoint, and one destination. Changing the
service, its server or endpoint, or its **Send to** address asks again and turns
the option off until you accept for the new configuration. Turning the option off
clears the consent. Turning it on for a second service replaces the consent, so
only the service it covers adds the text.

## Set up a sending service

The simplest configuration is a mailbox you already own as both the sender and
the destination. It needs no new domain, no DNS records, and no new paid plan:
the mailbox's own app password or authorization code is the only credential, and
the notification is an ordinary email to yourself. A verified sending domain
stays an optional upgrade for deliverability, not a prerequisite.

The steps below are what each provider documents for a third-party client at the
time of writing. Check the provider's page, because a provider can change its
relay or the name of its credential.

### Use Resend

1. Create a Resend account, then create an API key on the
   [API keys page](https://resend.com/docs/dashboard/api-keys/introduction).
   Resend does not show the key again after it is created.
2. In Koushi, add a service with **Service → Resend (email API)**, set **From
   address** and **Send to**, and paste the API key into **App password or API
   key**.
3. Send a test.

Resend's testing sender `onboarding@resend.dev` can send only to the email
address on your Resend account; to send anywhere else you must verify a domain
and use an address at that domain. The restriction and the resolution steps are
in Resend's
[403 Error Using resend.dev Domain](https://resend.com/docs/knowledge-base/403-error-resend-dev-domain).
Left blank, **Endpoint** uses the email API endpoint Resend documents.

### Use iCloud Mail

1. Create an app-specific password in your Apple Account, following Apple's
   [app-specific password steps](https://support.apple.com/en-us/102654). Copy
   the password.
2. In Koushi, add a service with **Service → SMTP relay**. Fill in **From
   address** first, then choose the **iCloud Mail** mailbox preset, which fills in
   the server Apple documents
   ([iCloud Mail server settings](https://support.apple.com/en-us/102525)).
3. Save the app-specific password, then send a test.

Apple issues app-specific passwords for accounts that use two-factor
authentication, and the password is only for this kind of app, not your Apple
Account password.

### Use Gmail

1. Turn on 2-Step Verification for your Google Account. Google offers app
   passwords only while it is on.
2. Create an app password following Google's
   [app password steps](https://support.google.com/accounts/answer/185833), and
   copy the 16-character password.
3. In Koushi, add a service with **Service → SMTP relay**. Fill in **From
   address** first, then choose the **Gmail** mailbox preset, which fills in the
   server Google documents
   ([Gmail IMAP, POP, and SMTP](https://developers.google.com/workspace/gmail/imap/imap-smtp)).
4. Save the app password, then send a test.

Google may not offer app passwords for every account, for example a managed
account whose administrator has turned them off, so the setting may be missing or
the password may be refused even with 2-Step Verification on.

### Use QQ Mail

1. Sign in to QQ Mail on the web.
2. Open **Settings**, then **Account and Security**.
3. Turn on the POP3/IMAP/SMTP service and complete the identity check the page
   asks for.
4. Copy the 16-character authorization code. QQ Mail's own page,
   [How to enable POP3/SMTP/IMAP and get an authorization code](https://help.mail.qq.com/detail/0/1087),
   has the full steps; the code is the password for a third-party client, never
   the QQ password. Changing the QQ password invalidates every authorization code
   ([QQ Mail client sign-in guide](https://help.mail.qq.com/detail/0/1088)).
5. In Koushi, add a service with **Service → SMTP relay**. Fill in **From
   address** first, then choose the **QQ Mail** mailbox preset, which fills in the
   relay QQ Mail documents
   ([Mac mail client settings](https://help.mail.qq.com/detail/108/1089/)), and
   save the authorization code.

QQ Mail can also send its own new-mail reminder to WeChat, so the same mailbox
can reach WeChat without a second service. That reminder is part of QQ Mail's own
settings, which Koushi does not configure; see the
[QQ Mail help centre](https://help.mail.qq.com/). It reaches WeChat only while
QQ Mail's reminder is set up and QQ Mail is delivering the message.

### Use another SMTP relay

Any SMTP relay can be entered by hand: with **Service → SMTP relay**, choose
**Other server** and fill in **Server**, **Port**, **Connection security**, and
**Username**, then save the password the relay documents. This covers a company
relay or a server you run yourself. A self-hosted server or a provider account
needs its host, its port, and its TLS mode; your provider or administrator has
those, and Koushi does not probe a server to guess them. Choose **TLS from the
first byte (usually port 465)** or **STARTTLS after connecting (usually port
587)** exactly as the relay documents, and use a **From address** the relay is
allowed to send as.

## Ask for help

Open the desktop **Help → Koushi Help** menu, including before sign-in, or
**App Settings → Help & About**. Choose **Copy GitHub URL**, then paste it into
ChatGPT or another AI assistant with your Koushi version, operating system, and
question. The button copies only the public repository URL. If copying fails,
select and copy the displayed URL manually.

The [guide index](README.md) explains how to find instructions for your installed
release. The desktop Help menu opens this short assistance dialog; the keyboard
shortcut reference is under **App Settings → Keyboard**.
