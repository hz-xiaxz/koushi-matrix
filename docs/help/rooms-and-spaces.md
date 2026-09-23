# Rooms and Spaces

[User guide and version selection](README.md)

## Navigate Home and Spaces

A room contains a conversation. A Space groups rooms; joining a Space does not
necessarily join every room it lists.

Choose **Home** in the left rail to see your account-level navigation, including
**Invites** and **Explore**. Choose a Space to see its rooms. If a conversation
is absent from a Space view, return to Home before assuming it has disappeared.

## Join a room

If you received an invitation:

1. Open **Home → Invites**.
2. Select the invitation and check the room and inviter.
3. Choose **Accept invite** to join, or **Decline invite** to reject it.

If you have a room address:

1. Open **Home → Explore**.
2. Enter the room address or Matrix link in the address field and choose
   **Preview**.
3. Review the preview and choose the available join action. Invite-only rooms
   require an invitation; a preview does not grant access.

Explore also lets you search a server's public room directory. This searches
rooms to join, whereas the top search field [searches messages](search.md).

## Create a room or Space

Use **Create room** in the sidebar header, enter a name, and review the privacy
and encryption choices before creating it. In the current creation dialog,
choosing a public room turns encryption off. Review the final options rather
than assuming every new room is encrypted.

Use **Create space**, the plus button near the bottom of the left rail, to
create a Space. Room and Space administration actions depend on your role.

## Start a direct message

Select **New DM** in the sidebar header. Enter the person's full Matrix ID in
**Matrix user ID**, check the address, and choose **Start DM**.
A direct message is still a Matrix room; the other person may need to accept
an invitation before participating.

## Invite someone to a room

1. Open the room and select **Room info** in its header.
2. Choose **Invite people**, search for or enter the person's Matrix ID, and
   select the intended person.
3. Review the offered scope and history options, then choose **Send invite**.

If the action is unavailable, your role or the current room state may not allow
it. Room history visibility and room entry rules are separate settings.
**Since invite**, **Since join**, and **Shared history** describe different
history access. In encrypted rooms, reading older history also requires the
appropriate encryption keys. Changing a setting does not revoke events or keys
already shared.

## Conversation list sections

The conversation list shows **Rooms** above **DMs**. Each heading can be
collapsed and sorted from its own menu. The number to the right of a heading is
its unread total for the current Home or Space view; it turns into a red badge
when there is something unread, shows `99+` above 99, and disappears at zero. It
stays on the Rust-reported total for the whole view, so filtering the list or
collapsing the section does not change it.

A **Low priority** section appears below **DMs** when any conversation in the
current view carries the low-priority tag. Low-priority rooms and DMs are listed
there instead of in **Rooms** or **DMs**. Set or clear the tag from a
conversation's context menu; the section is hidden when it is empty and can be
collapsed like the others.

Low priority quiets a conversation without muting or reading it: it raises no
desktop notification or sound, and it is excluded from the Dock or taskbar
badge, the Home and Space rail counts, and the **Rooms** and **DMs** heading
badges, mentions included. The conversation's own row still shows its real
unread count, nothing is marked as read, and removing the tag restores its
contribution without replaying old notifications.

## Room information and notifications

Open **Room info** to find members, files, room notification options, and settings
available to your role. Room notification choices include **All messages**,
**Mentions only**, and **Mute**. Device notification permission and global
notification settings can also affect whether a desktop notification appears.
**Mute** suppresses desktop notifications and sounds and excludes that room from
notification badge totals, even if it still has unread messages or mentions.
Muting does not mark messages as read. Room notification changes made in another
Matrix client are reflected after synchronization.

## Download room history

Open the room, select **Room info**, and choose **Download history** under
**Download history**. Koushi saves the messages as one JSON file in the format
of Element's chat export, so scripts and tools that read Element exports can
read it. It is not a backup: neither Koushi nor Element can import it back.

1. Choose the range:
   - **All available history** saves every event your account can read.
   - **Period** saves the events from the start date through the end date,
     inclusive. The dialog shows the time zone it uses for the dates, which is
     your computer's time zone.
2. Select **Save**, then choose where to save the file.
3. The dialog shows how many events have been read and saved. Closing it does
   not stop the download; **Room info** keeps showing the progress. Select
   **Stop** to cancel.

What can be saved depends on your permission to read the room's history, on
what the server still keeps, and on which messages this device can decrypt.
Attachments keep their names and references, but the files themselves are
not downloaded.

When the download finishes, Koushi reports how many events were saved. If some
encrypted messages could not be decrypted, it also reports how many were saved
without their content. If you stop the download or it fails, no file is saved;
a file that was already at the chosen location is left unchanged.

**Encrypted messages are saved as plain text.** Anyone who can open the file
can read them, so keep it somewhere safe.
