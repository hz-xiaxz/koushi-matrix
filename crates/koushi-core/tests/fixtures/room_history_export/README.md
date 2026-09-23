# Room-history export compatibility fixtures (#59)

Synthetic input and expected output for the Element-compatible chat-export
JSON. Every user, room, event, and media identifier is under
`example.invalid`; message bodies and filenames are neutral examples.

## Compatibility baseline

The expected output follows these upstream revisions:

- element-web `c9cff69c74d5faa4606167863e066ef02c0bae0d`
  (`apps/web/src/utils/exportUtils/JSONExport.ts`, `Exporter.ts`,
  `apps/web/src/events/EventTileFactory.tsx`, `apps/web/src/TextForEvent.tsx`)
- matrix-js-sdk `b08a603df74fbb7e97cbfe83097b004ff4122b93`
  (`src/models/event.ts`: `getEffectiveEvent`, `isRelation`,
  `setClearDataForDecryptionFailure`)

`element_expected.json` was derived by applying those sources by hand to
`source_events.json`. It was not produced by running Element: the export needs
a full Element Web client and test environment. Re-derive it when the baseline
revisions change.

## What the expected output encodes

- `messages` contains `getEffectiveEvent()` for each event that passes
  `haveRendererForEvent(event, client, false)`, in timeline order. Each
  `source_events.json` entry's position in `events` gives that order.
- Decrypted events are the Matrix crypto crate's decrypted event. That is the
  object matrix-js-sdk's Rust crypto backend stores as `clearEvent`.
- Undecryptable events become `type: "m.room.message"` with
  `content.msgtype: "m.bad.encrypted"` and the `** Unable to decrypt: … **`
  body. Wire content keys outside the encryption schema, such as
  `m.relates_to`, are copied into the content.
- A redacted `m.room.encrypted` event stays the pruned wire event.
  matrix-js-sdk does not decrypt redacted events.
- Edits are not applied to their originals. Element's exporter maps freshly
  fetched `/messages` events, and `m.replace` events have no renderer.
- Reactions, redaction events, `m.room.server_acl`, null rejoins,
  power-level events without a user change, create events without a
  predecessor, and verification requests for other users have no renderer and
  are excluded.

## Normalized in comparisons

- `export_date`: the export time rendered by
  `Intl.DateTimeFormat(locale, { year, month, day: "numeric" })` in the
  user's time zone. It is tested separately.
- Object key order: `JSON.stringify` keeps the server's insertion order and
  Koushi writes keys sorted. Tests compare parsed JSON values.
- The undecryptable-event reason text: Element uses the decryption error of
  the moment, and Koushi maps the SDK failure reason to the closest
  matrix-js-sdk message.
