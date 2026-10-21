// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SessionAccessTokenSection } from "./SessionAccessTokenSection";

const scope = { sessionKey: "session-generation-1", homeserver: "https://example.test", userId: "@alice:example.test", deviceId: "DEVICE_A", available: true };
const syntheticToken = (suffix = "one") => `synthetic-test-credential-${suffix}`;
const reply = (suffix = "one") => ({ access_token: syntheticToken(suffix), homeserver: scope.homeserver, user_id: scope.userId, device_id: scope.deviceId });
const tokenVisible = (container: HTMLElement, suffix = "one") => container.textContent?.includes(syntheticToken(suffix)) ?? false;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function clipboard(writeText = vi.fn().mockResolvedValue(undefined)) {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  return writeText;
}

async function click(name: string) {
  await act(async () => { fireEvent.click(screen.getByRole("button", { name })); });
}

describe("explicit current-session access token controls", () => {
  it("retrieves only on explicit action, reveals in DOM, hides and remasks", async () => {
    const retrieve = vi.fn().mockResolvedValue(reply());
    const { container } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
    expect(retrieve).not.toHaveBeenCalled();
    expect(tokenVisible(container)).toBe(false);
    await click("Reveal");
    expect(retrieve).toHaveBeenCalledTimes(1);
    expect(tokenVisible(container)).toBe(true);
    await click("Hide");
    expect(tokenVisible(container)).toBe(false);
    expect(retrieve).toHaveBeenCalledTimes(1);
  });

  it("retrieves afresh for copy rather than copying the revealed value", async () => {
    const writeText = clipboard();
    const retrieve = vi.fn().mockResolvedValueOnce(reply()).mockResolvedValueOnce(reply("two"));
    const { container } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
    await click("Reveal");
    await click("Copy");
    expect(retrieve).toHaveBeenCalledTimes(2);
    expect(writeText.mock.calls.length).toBe(1);
    expect(writeText.mock.calls[0]?.[0] === syntheticToken("two")).toBe(true);
    expect(tokenVisible(container, "two")).toBe(false);
  });

  it.each(["sessionKey", "homeserver", "userId", "deviceId", "available"] as const)("fences a late copy response when %s changes", async (field) => {
    const writeText = clipboard();
    const pending = deferred<ReturnType<typeof reply>>();
    const retrieve = vi.fn().mockReturnValue(pending.promise);
    const { container, rerender } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
    await click("Copy");
    const next = { ...scope, [field]: field === "available" ? false : `${scope[field]}-changed` };
    rerender(<SessionAccessTokenSection {...next} retrieve={retrieve} />);
    await act(async () => { pending.resolve(reply()); });
    expect(writeText).not.toHaveBeenCalled();
    expect(tokenVisible(container)).toBe(false);
  });

  it("ignores retrieval completing after unmount", async () => {
    const writeText = clipboard();
    const pending = deferred<ReturnType<typeof reply>>();
    const { container, unmount } = render(<SessionAccessTokenSection {...scope} retrieve={() => pending.promise} />);
    await click("Copy");
    unmount();
    await act(async () => { pending.resolve(reply()); });
    expect(writeText).not.toHaveBeenCalled();
    expect(tokenVisible(container)).toBe(false);
  });

  it("removes a revealed token immediately when the session locks", async () => {
    const retrieve = vi.fn().mockResolvedValue(reply());
    const { container, rerender } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
    await click("Reveal");
    rerender(<SessionAccessTokenSection {...scope} available={false} retrieve={retrieve} />);
    expect(tokenVisible(container)).toBe(false);
    expect((screen.getByRole("button", { name: "Reveal" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Copy" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it.each(["homeserver", "user_id", "device_id"] as const)("rejects a response with a mismatched %s", async (field) => {
    const writeText = clipboard();
    const retrieve = vi.fn().mockResolvedValue({ ...reply(), [field]: "wrong-scope" });
    const { container } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
    await click("Reveal");
    await click("Copy");
    expect(tokenVisible(container)).toBe(false);
    expect(writeText).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("Could not retrieve");
  });

  it("uses a fixed localized clipboard failure without revealing secret-bearing errors", async () => {
    clipboard(vi.fn().mockRejectedValue(new Error(syntheticToken())));
    const { container } = render(<SessionAccessTokenSection {...scope} retrieve={vi.fn().mockResolvedValue(reply())} />);
    await click("Copy");
    expect(screen.getByRole("status").textContent).toBe("Could not copy the access token.");
    expect(tokenVisible(container)).toBe(false);
  });

  it("uses a fixed localized retrieval failure", async () => {
    const { container } = render(<SessionAccessTokenSection {...scope} retrieve={vi.fn().mockRejectedValue(new Error(syntheticToken()))} />);
    await click("Reveal");
    expect(screen.getByRole("status").textContent).toBe("Could not retrieve the access token.");
    expect(tokenVisible(container)).toBe(false);
  });
});

it("Hide cancels a pending reveal without letting its late result reappear", async () => {
  const pending = deferred<ReturnType<typeof reply>>();
  const { container } = render(<SessionAccessTokenSection {...scope} retrieve={() => pending.promise} />);
  await click("Reveal");
  await click("Hide");
  await act(async () => { pending.resolve(reply()); });
  expect(tokenVisible(container)).toBe(false);
  expect(screen.getByRole("button", { name: "Reveal" })).toBeTruthy();
});

it("clears the revealed DOM node itself on unmount", async () => {
  const { container, unmount } = render(<SessionAccessTokenSection {...scope} retrieve={vi.fn().mockResolvedValue(reply())} />);
  await click("Reveal");
  const node = container.querySelector("code")!;
  expect(Boolean(node.textContent)).toBe(true);
  unmount();
  expect(Boolean(node.textContent)).toBe(false);
});

it.each(["sessionKey", "homeserver", "userId", "deviceId", "available"] as const)("fences a late reveal response when %s changes", async (field) => {
  const pending = deferred<ReturnType<typeof reply>>();
  const retrieve = vi.fn().mockReturnValue(pending.promise);
  const { container, rerender } = render(<SessionAccessTokenSection {...scope} retrieve={retrieve} />);
  await click("Reveal");
  const next = { ...scope, [field]: field === "available" ? false : `${scope[field]}-changed` };
  rerender(<SessionAccessTokenSection {...next} retrieve={retrieve} />);
  await act(async () => { pending.resolve(reply()); });
  expect(tokenVisible(container)).toBe(false);
});

it("does not retrieve or copy while unavailable", async () => {
  const writeText = clipboard();
  const retrieve = vi.fn().mockResolvedValue(reply());
  render(<SessionAccessTokenSection {...scope} available={false} retrieve={retrieve} />);
  await click("Reveal");
  await click("Copy");
  expect(retrieve).not.toHaveBeenCalled();
  expect(writeText).not.toHaveBeenCalled();
});
