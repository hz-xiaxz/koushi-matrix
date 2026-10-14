// @vitest-environment jsdom
import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useRoomAddressPreview } from "./useRoomAddressPreview";
import type { RoomAddressPreview } from "../domain/types";

const preview = (localpart: string): RoomAddressPreview => ({
  localpart, full_alias: `#${localpart}:example.invalid`, error: null, server_name: "example.invalid"
});

test("returns only Rust previews for the current account, session and draft", async () => {
  const pending: Array<(value: RoomAddressPreview) => void> = [];
  const api = { previewRoomAddress: vi.fn(() => new Promise<RoomAddressPreview>(resolve => pending.push(resolve))) };
  const { result, rerender } = renderHook(({ session, alias, account, ready }) =>
    useRoomAddressPreview(api, session, alias, account, ready, true),
    { initialProps: { session: 1, alias: null as string | null, account: "account-a", ready: true } });
  rerender({ session: 1, alias: "manual", account: "account-a", ready: true });
  await act(async () => pending[0]!(preview("room-3fa9c2")));
  expect(result.current).toBeNull();
  await act(async () => pending[1]!(preview("manual")));
  await waitFor(() => expect(result.current?.localpart).toBe("manual"));
  expect(api.previewRoomAddress).toHaveBeenLastCalledWith(1, "manual");
  rerender({ session: 1, alias: "manual", account: "account-b", ready: true });
  expect(result.current).toBeNull();
});

test("re-requests the default once the create draft is open (#1416)", async () => {
  const pending: Array<(value: RoomAddressPreview) => void> = [];
  const api = { previewRoomAddress: vi.fn(() => new Promise<RoomAddressPreview>(resolve => pending.push(resolve))) };
  const { result, rerender } = renderHook(({ ready }) =>
    useRoomAddressPreview(api, 4, null, "account-a", ready, true),
    { initialProps: { ready: false } });
  rerender({ ready: true });
  expect(api.previewRoomAddress).toHaveBeenCalledTimes(2);
  await act(async () => pending[0]!({ localpart: "", full_alias: null, error: "empty", server_name: "example.invalid" }));
  expect(result.current).toBeNull();
  await act(async () => pending[1]!(preview("room-0b1c2d")));
  await waitFor(() => expect(result.current?.localpart).toBe("room-0b1c2d"));
});
