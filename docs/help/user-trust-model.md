# User Trust Model

[User guide and version selection](README.md)

Koushi separates user trust, device state, and the final send decision. This
keeps the normal Matrix case from looking more dangerous than it is.

## User trust

### Unverified

This is the normal state for people you have not checked through another
channel. Messages can still be encrypted and sent. Verifying another person is
optional; Koushi does not show a warning or prompt just because you have not
done it.

### Verified

The user's identity has been checked through another channel. Use this for
people or rooms where impersonation resistance matters more than convenience.

### Identity reset

The user was verified before, but their current identity is different. This does
not prove compromise, but it means the previous verification can no longer be
used. Verify again, or explicitly forget the previous verification and treat the
user as unverified.

## Device state

Device state describes whether a device is cross-signed by its owner. It is not
the same as whether you have verified that user.

- **Cross-signed:** the owner identity signs this device.
- **Not cross-signed:** the device key exists, but the owner identity has not
  signed it.
- **Blocked:** the device is explicitly rejected.

## Effective trust

Effective trust is Koushi's send decision after combining user trust, device
state, blocked devices, and identity-reset warnings. A user can be unverified
while their devices are cross-signed by that user's own identity; that is still
different from a user you have verified yourself.


## See a person's security details

Open a person's **User info** and look under **Security**. It shows two
separate rows:

- **Their devices** — whether the person has confirmed (signed) each of their
  encryption devices with their own identity in their own app. If some are not
  yet confirmed and you are concerned, ask them to confirm their devices in
  their app. Verifying the person yourself does not confirm those devices.
- **Your verification** — whether you verified that the account belongs to
  the person you know. This stays **Verified by you** when the person adds a
  new device. **Your verification no longer applies** is the only state
  highlighted for attention: you verified the person before, but their
  identity or yours has been reset since, so verification needs to be
  repeated. If you never verified the person, an identity change stays
  neutral.

To verify the person yourself, choose **Verify user** (or **Verify again**
when your earlier verification no longer applies) under **Your
verification**. Koushi first shows which direct chat it will use for the
request — your existing chat with them (inviting them back if they left it),
or a new encrypted chat — and sends nothing until you
choose **Send request**. While another verification is in progress, Koushi
says so instead of offering the action. When they accept in their app, compare the emoji
with them in person or over another trusted channel and choose **They match**
or **They don't match**. Verifying them does not confirm devices they have
not confirmed themselves. If this session does not have your own
cross-signing keys, Koushi explains that instead of offering the action.

Choose **Details** on a row for a short explanation, device counts, and a
device list. Opening the details changes nothing. **Status unavailable** means
Koushi could not retrieve the person's keys; it is not a confirmation. These
details are about the person's keys, not about whether a conversation is
encrypted.

## Let someone know about unconfirmed devices

In a conversation, a small muted **ⓘ** next to a sender's name means that
person has not confirmed every device signed in to their account. It is a
hint, not an alarm: their messages still arrive as usual. Your own messages
and people you ignore never show it.

Choose the **ⓘ** to see what it means. From there you can choose **View in
User info** for the details, or **Let them know** to open your direct chat
with them with a short, editable advice message already in the composer.
Pick the message language first; Koushi has drafts in every language it
supports. Nothing is sent until you send it yourself. If you have no direct
chat with them yet, Koushi asks before creating one. If you already have
unsent text in that chat, Koushi keeps your text and does not add the advice
message.
