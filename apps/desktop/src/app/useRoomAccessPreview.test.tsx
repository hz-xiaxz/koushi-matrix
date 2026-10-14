// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import type { RoomAccessDraftScope, RoomAccessPreview } from "../domain/types";
import { useCreateRoomAccessPreview, useRoomAccessPreview } from "./useRoomAccessPreview";

const scope: RoomAccessDraftScope = { kind: "room", roomId: "!room:example.invalid" };

function preview(joinMessageId: RoomAccessPreview["outcome"]["join"]["messageId"]): RoomAccessPreview {
  return {
    scope,
    context: "access",
    confirmed: false,
    outcome: {
      join: { messageId: joinMessageId },
      history: { messageId: "room.accessOutcomeHistoryShared" },
      encryption: { messageId: "room.accessOutcomeNotEncrypted" },
      directory: { messageId: "room.accessOutcomeDirectoryPrivate" },
      nonRetroactive: { messageId: "room.historyNonRetroactive" }
    }
  };
}

test("an older preview result never replaces newer details (#1177)", async () => {
  const pending: Array<(value: RoomAccessPreview) => void> = [];
  const api = {
    previewRoomAccess: vi.fn(() => new Promise<RoomAccessPreview>((resolve) => pending.push(resolve)))
  };
  const { result, rerender } = renderHook(
    ({ generation }) => useRoomAccessPreview(api, scope, "access", "draft:1", generation, true),
    { initialProps: { generation: 1 } }
  );
  // A second identity (a confirmed-property advance with no draft change)
  // starts a newer request, and its effect cleanup retires the first.
  rerender({ generation: 2 });
  await act(async () => {
    pending[1]!(preview("room.accessOutcomeJoinPublic"));
  });
  await waitFor(() =>
    expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinPublic")
  );
  // The older result resolves last; it must not overwrite the newer details.
  await act(async () => {
    pending[0]!(preview("room.accessOutcomeJoinInvite"));
  });
  expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinPublic");
});

test("leaving the panel clears the preview", async () => {
  const api = {
    previewRoomAccess: vi.fn(() => Promise.resolve(preview("room.accessOutcomeJoinPublic")))
  };
  const { result, rerender } = renderHook(
    ({ enabled }) => useRoomAccessPreview(api, scope, "access", "draft:1", 1, enabled),
    { initialProps: { enabled: true } }
  );
  await waitFor(() =>
    expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinPublic")
  );
  rerender({ enabled: false });
  expect(result.current).toBeNull();
});

test("an unrelated state generation keeps the shown preview while revalidating (#1417)", async () => {
  const pending: Array<(value: RoomAccessPreview) => void> = [];
  const api = {
    previewRoomAccess: vi.fn(() => new Promise<RoomAccessPreview>((resolve) => pending.push(resolve)))
  };
  const { result, rerender } = renderHook(
    ({ generation }) => useRoomAccessPreview(api, scope, "access", "draft:1", generation, true),
    { initialProps: { generation: 1 } }
  );
  await act(async () => {
    pending[0]!(preview("room.accessOutcomeJoinPublic"));
  });
  await waitFor(() =>
    expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinPublic")
  );
  // A snapshot advance that does not change this panel's selection (a sync,
  // a typing notice, another room) must not blank the details: blanking them
  // collapses the panel, which makes it flicker and jumps the scroll position.
  rerender({ generation: 2 });
  expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinPublic");
  expect(api.previewRoomAccess).toHaveBeenCalledTimes(2);
  await act(async () => {
    pending[1]!(preview("room.accessOutcomeJoinInvite"));
  });
  expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinInvite");
});

test("a selection change never shows the previous selection's preview (#1177, #1417)", async () => {
  const pending: Array<(value: RoomAccessPreview) => void> = [];
  const api = {
    previewRoomAccess: vi.fn(() => new Promise<RoomAccessPreview>((resolve) => pending.push(resolve)))
  };
  const { result, rerender } = renderHook(
    ({ selection, generation }) =>
      useRoomAccessPreview(api, scope, "access", selection, generation, true),
    { initialProps: { selection: "draft:1", generation: 1 } }
  );
  await act(async () => {
    pending[0]!(preview("room.accessOutcomeJoinPublic"));
  });
  await waitFor(() => expect(result.current).not.toBeNull());
  rerender({ selection: "draft:2", generation: 2 });
  expect(result.current).toBeNull();
  await act(async () => {
    pending[1]!(preview("room.accessOutcomeJoinInvite"));
  });
  expect(result.current?.outcome.join.messageId).toBe("room.accessOutcomeJoinInvite");
});

test("the create preview survives an unrelated state generation (#1417)", async () => {
  const createScope: RoomAccessDraftScope = { kind: "create", sessionId: 3 };
  const input = { visibility: "private" as const, invitedOnly: false, encrypted: true, parentSpaceId: null };
  const pending: Array<(value: unknown) => void> = [];
  const api = {
    previewCreateRoomAccess: vi.fn(() => new Promise((resolve) => pending.push(resolve)))
  };
  const { result, rerender } = renderHook(
    ({ generation }) =>
      useCreateRoomAccessPreview(
        api as never,
        createScope,
        input,
        "draft:4",
        generation,
        true
      ),
    { initialProps: { generation: 1 } }
  );
  const shown = { scope: createScope, confirmed: false, effectiveRule: "invite" };
  await act(async () => {
    pending[0]!(shown);
  });
  await waitFor(() => expect(result.current).toEqual(shown));
  rerender({ generation: 2 });
  expect(result.current).toEqual(shown);
});
