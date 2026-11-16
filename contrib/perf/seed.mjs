// Seed a disposable local tuwunel with a synthetic, realistic-scale account.
// Usage: PATH=/tmp/koushi-desktop-local-qa-bin:$PATH node contrib/perf/seed.mjs <workdir>
// See README.md in this directory. Every name, message and account is synthetic.
import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const LIB = pathToFileURL(join(REPO, "scripts/lib/local-homeserver-qa.mjs")).href;
const { startHomeserver, stopProcess, tuwunelConfig, waitForHomeserver, registerUser } = await import(LIB);

if (!process.argv[2]) {
  console.error("usage: node contrib/perf/seed.mjs <workdir>");
  process.exit(2);
}
const work = resolve(process.argv[2]);
const PORT = Number(process.env.PERF_PORT ?? 18448);
const SMALL_ROOMS = Number(process.env.PERF_SMALL_ROOMS ?? 150);
const SMALL_MSGS = Number(process.env.PERF_SMALL_MSGS ?? 40);
const BIG_ROOMS = Number(process.env.PERF_BIG_ROOMS ?? 3);
const BIG_MSGS = Number(process.env.PERF_BIG_MSGS ?? 3000);
const DMS = Number(process.env.PERF_DMS ?? 20);
const HELPERS = 12;

if (existsSync(join(work, "state.json"))) throw new Error("already seeded");
mkdirSync(join(work, "hs"), { recursive: true });
const homeserver = `http://127.0.0.1:${PORT}`;
const configPath = join(work, "tuwunel.toml");
writeFileSync(configPath, tuwunelConfig({ serverName: `localhost:${PORT}`, port: PORT, dataDir: join(work, "hs") }));
const server = startHomeserver("tuwunel", configPath, join(work, "tuwunel.log"));
await waitForHomeserver(homeserver, server, 60000, join(work, "tuwunel.log"));

let txn = 0;
async function api(token, method, path, body) {
  for (let attempt = 0; ; attempt++) {
    const r = await fetch(`${homeserver}/_matrix/client/v3${path}`, {
      method,
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
    const text = await r.text();
    if (r.status === 429 && attempt < 20) {
      await new Promise((res) => setTimeout(res, 200));
      continue;
    }
    if (!r.ok) throw new Error(`${method} ${path.split("/")[1]} HTTP ${r.status}: ${text.slice(0, 200)}`);
    return text ? JSON.parse(text) : {};
  }
}
const enc = encodeURIComponent;
const send = (tok, room, content, type = "m.room.message") =>
  api(tok, "PUT", `/rooms/${enc(room)}/send/${type}/perf${++txn}`, content);
const state = (tok, room, type, key, content) =>
  api(tok, "PUT", `/rooms/${enc(room)}/state/${type}/${enc(key)}`, content);

const sfx = Date.now().toString(36);
const primary = await registerUser(homeserver, `perf_main_${sfx}`, `perf-main-${sfx}`);
const P = { token: primary.access_token, userId: primary.user_id };
await api(P.token, "PUT", `/profile/${enc(P.userId)}/displayname`, { displayname: "Perf Main" });
const helpers = [];
for (let i = 0; i < HELPERS; i++) {
  const r = await registerUser(homeserver, `perf_h${i}_${sfx}`, `perf-h-${sfx}`);
  const h = { token: r.access_token, userId: r.user_id };
  await api(h.token, "PUT", `/profile/${enc(h.userId)}/displayname`, { displayname: `Perf Helper ${i}` });
  helpers.push(h);
}

const LOREM = "The quick brown fox jumps over the lazy dog while synthetic benchmark traffic keeps flowing through the room";
function body(tag, i) {
  const kind = i % 10;
  if (kind === 3) return { msgtype: "m.text", body: `${tag} m${i} multi\nline two\nline three ${LOREM}` };
  if (kind === 6)
    return {
      msgtype: "m.text",
      body: `${tag} m${i} **bold** and code`,
      format: "org.matrix.custom.html",
      formatted_body: `${tag} m${i} <strong>bold</strong> <code>code()</code><blockquote>quoted ${LOREM}</blockquote><ul><li>one</li><li>two</li></ul>`
    };
  if (kind === 8) return { msgtype: "m.text", body: `${tag} m${i} https://example.org/page/${i} ${LOREM.slice(0, 40)}` };
  return { msgtype: "m.text", body: `${tag} m${i} ${LOREM.slice(0, 20 + ((i * 7) % 90))}` };
}

async function makeRoom(name, members, extra = {}) {
  const room = await api(P.token, "POST", "/createRoom", { name, preset: "private_chat", invite: members.map((m) => m.userId), ...extra });
  for (const m of members) await api(m.token, "POST", `/join/${enc(room.room_id)}`, {});
  return room.room_id;
}

async function fill(roomId, tag, count, members) {
  const senders = [P, ...members];
  const ids = [];
  let last = null;
  for (let i = 0; i < count; i++) {
    const s = senders[(i * 5 + 3) % senders.length];
    const content = body(tag, i);
    if (i % 25 === 24 && ids.length) content["m.relates_to"] = { "m.in_reply_to": { event_id: ids[ids.length - 3] } };
    const r = await send(s.token, roomId, content);
    ids.push(r.event_id);
    last = { i, eventId: r.event_id };
    if (i % 40 === 20) {
      const rs = senders[(i + 1) % senders.length];
      await send(rs.token, roomId, { "m.relates_to": { rel_type: "m.annotation", event_id: r.event_id, key: "👍" } }, "m.reaction");
    }
  }
  // Main user has read up to ~80% of the room, so rooms carry unread counts.
  const readIdx = Math.floor(ids.length * 0.8);
  if (ids[readIdx]) await api(P.token, "POST", `/rooms/${enc(roomId)}/read_markers`, { "m.fully_read": ids[readIdx], "m.read": ids[readIdx] });
  return { count, lastMarker: `${tag} m${count - 1}` };
}

async function pool(items, n, fn) {
  let next = 0;
  await Promise.all(Array.from({ length: n }, async () => {
    while (next < items.length) {
      const idx = next++;
      await fn(items[idx], idx);
    }
  }));
}

const t0 = Date.now();
const rooms = [];
const smallSpecs = Array.from({ length: SMALL_ROOMS }, (_, i) => i);
await pool(smallSpecs, 8, async (i) => {
  const name = `Perf Room ${String(i).padStart(3, "0")}`;
  const members = [helpers[i % HELPERS], helpers[(i + 3) % HELPERS], helpers[(i + 7) % HELPERS]];
  const id = await makeRoom(name, members, { topic: `Synthetic room ${i}` });
  const tag = `[r${String(i).padStart(3, "0")}]`;
  const f = await fill(id, tag, SMALL_MSGS, members);
  rooms.push({ kind: "small", name, id, tag, ...f });
});
console.log(`small rooms done ${(Date.now() - t0) / 1000}s`);

const dmAccount = {};
await pool(Array.from({ length: DMS }, (_, i) => i), 4, async (i) => {
  const h = helpers[i % HELPERS];
  // DMs created by the helper so the main user sees the helper's name.
  const room = await api(h.token, "POST", "/createRoom", { preset: "trusted_private_chat", is_direct: true, invite: [P.userId] });
  await api(P.token, "POST", `/join/${enc(room.room_id)}`, {});
  (dmAccount[h.userId] ??= []).push(room.room_id);
  const tag = `[d${String(i).padStart(2, "0")}]`;
  const f = await fill(room.room_id, tag, 30, [h]);
  rooms.push({ kind: "dm", name: null, helper: i % HELPERS, id: room.room_id, tag, ...f });
});
await api(P.token, "PUT", `/user/${enc(P.userId)}/account_data/m.direct`, dmAccount);
console.log(`dms done ${(Date.now() - t0) / 1000}s`);

await pool(Array.from({ length: BIG_ROOMS }, (_, i) => i), BIG_ROOMS, async (i) => {
  const name = `Perf Big ${String.fromCharCode(65 + i)}`;
  const id = await makeRoom(name, helpers, { topic: "Large synthetic room" });
  const tag = `[b${i}]`;
  const f = await fill(id, tag, BIG_MSGS, helpers);
  rooms.push({ kind: "big", name, id, tag, ...f });
});
console.log(`big rooms done ${(Date.now() - t0) / 1000}s`);

const spaces = [];
for (let s = 0; s < 2; s++) {
  const name = `Perf Space ${s}`;
  const space = await api(P.token, "POST", "/createRoom", { name, preset: "private_chat", creation_content: { type: "m.space" } });
  const children = rooms.filter((r) => r.kind === "small").slice(s * 30, s * 30 + 30);
  for (const c of children) {
    await state(P.token, space.room_id, "m.space.child", c.id, { via: [`localhost:${PORT}`] });
  }
  spaces.push({ name, id: space.room_id, children: children.map((c) => c.name) });
}

writeFileSync(
  join(work, "state.json"),
  JSON.stringify({ homeserver, port: PORT, username: `perf_main_${sfx}`, password: `perf-main-${sfx}`, userId: P.userId, rooms, spaces }, null, 1)
);
console.log(`seed done ${(Date.now() - t0) / 1000}s rooms=${rooms.length}`);
await stopProcess(server);
