import { createRoom, joinRoom, registerUser } from "./local-homeserver-qa.mjs";

// Thirty joined users before B joins: the real server must report31, rather
// than letting a lazy member cache accidentally select the small-room default.
export async function seedHistoryKeyRecoveryFixture({ homeserver, owner, regular, runId }) {
  const room = await createRoom(homeserver, owner.access_token, {
    preset: "public_chat",
    name: "Synthetic history recovery",
    initial_state: [{ type: "m.room.history_visibility", state_key: "", content: { history_visibility: "shared" } }]
  });
  const roomId = room.room_id;
  if (typeof roomId !== "string") throw new Error("history recovery fixture room missing");
  await joinRoom(homeserver, regular.access_token, roomId);
  for (let offset = 0; offset < 28; offset += 4) {
    await Promise.all(Array.from({ length: 4 }, async (_, index) => {
      const ordinal = offset + index;
      const member = await registerUser(homeserver, `qa_history_${runId}_${ordinal}`, `synthetic-history-${runId}-${ordinal}`);
      await joinRoom(homeserver, member.access_token, roomId);
    }));
  }
  return roomId;
}
