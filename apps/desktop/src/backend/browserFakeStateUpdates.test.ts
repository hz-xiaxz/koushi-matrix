import { expect, test, vi } from "vitest";

import { createBrowserFakeApi } from "./browserFakeApi";
import { listenBrowserFakeStateUpdates } from "./browserFakeStateUpdates";

test("browser fake emits one versioned delta before resolving a command receipt", async () => {
  const listener = vi.fn();
  const unlisten = await listenBrowserFakeStateUpdates(listener);
  const api = createBrowserFakeApi();

  const receipt = await api.setDisplayName("Updated Browser User");

  expect(listener).toHaveBeenCalledOnce();
  expect(listener).toHaveBeenCalledWith(
    expect.objectContaining({
      protocol_version: 1,
      kind: "delta",
      generation: receipt.admittedGeneration
    })
  );
  unlisten();
});

test("whole-session replacements continue the published generation", async () => {
  const listener = vi.fn();
  const unlisten = await listenBrowserFakeStateUpdates(listener);
  const api = createBrowserFakeApi();

  const first = await api.setDisplayName("generation one");
  const oidc = await api.completeOidcLogin("https://example.invalid", "callback");
  const switched = await api.switchAccount((await api.listSavedSessions())[1]);

  expect([
    first.admittedGeneration,
    oidc.publishedGeneration,
    switched.publishedGeneration
  ]).toEqual([1, 2, 3]);

  const locked = createBrowserFakeApi({ session: "locked" });
  const beforeReauth = await locked.updateSettings({ appearance: { theme: "dark" } });
  const reauthed = await locked.submitSoftLogoutReauth("test-only");
  expect([beforeReauth.admittedGeneration, reauthed.publishedGeneration]).toEqual([1, 2]);
  expect(listener.mock.calls.map(([update]) => update.generation)).toEqual([1, 2, 3, 1, 2]);
  unlisten();
});
