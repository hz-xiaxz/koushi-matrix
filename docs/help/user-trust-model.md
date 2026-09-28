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
  new device. **Identity changed after you verified it** is the only state
  highlighted for attention; verification needs to be repeated. If you never
  verified the person, an identity change stays neutral.

Choose **Details** on a row for a short explanation, device counts, and a
device list. Opening the details changes nothing. **Status unavailable** means
Koushi could not retrieve the person's keys; it is not a confirmation. These
details are about the person's keys, not about whether a conversation is
encrypted.
