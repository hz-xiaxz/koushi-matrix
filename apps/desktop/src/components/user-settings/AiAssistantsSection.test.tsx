// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AccountTabSummary, McpAssistantsState } from "../../domain/types";
import { AiAssistantsSection } from "./AiAssistantsSection";

const fixture = vi.hoisted(() => ({
  listeners: new Set<(state: McpAssistantsState) => void>(),
  getState: vi.fn(),
  setPermission: vi.fn(),
  remove: vi.fn()
}));
vi.mock("../../backend/runtimeEnvironment", () => ({ isTauriRuntime: () => true }));
vi.mock("../../backend/appRuntime", () => ({
  api: {
    getMcpAssistantsState: fixture.getState,
    setMcpPermission: fixture.setPermission,
    removeMcpAssistant: fixture.remove
  }
}));
vi.mock("../../backend/desktopEventRuntime", () => ({ desktopEventPort: {
  listenMcpAssistants: async (listener: (state: McpAssistantsState) => void) => {
    fixture.listeners.add(listener);
    return () => fixture.listeners.delete(listener);
  }
} }));

const WORK = "account:@member1:example.invalid";
const HOME = "account:@member2:example.invalid";
const tab = (id: string, name: string) => ({
  id,
  accountKey: id.replace("account:", ""),
  homeserver: "example.invalid",
  displayName: name,
  avatarSourceRef: null,
  status: "ready",
  unreadCount: 0
}) as AccountTabSummary;
const tabs = [tab(WORK, "Synthetic Workspace"), tab(HOME, "Member Home")];

function state(overrides: Partial<McpAssistantsState> = {}): McpAssistantsState {
  return {
    bridge: "running",
    store: "ok",
    launcher: { state: "available", path: "/opt/koushi/koushi-mcp" },
    pending: [],
    assistants: [{
      client_name: "claude-code",
      client_version: "2.1.0",
      last_used_ms: null,
      accounts: [{ account_id: WORK, read: true, send: false, attachments: false, denied: ["send"] }]
    }],
    registrations: [
      { client: "claude_code", client_name: "claude-code", kind: "command", text: "claude mcp add --scope user koushi -- '/opt/koushi/koushi-mcp' --client-name claude-code", config_hint: null },
      { client: "deepseek_harness", client_name: "deepseek-harness", kind: "yaml", text: "- id: mcp-koushi\n", config_hint: "~/.dsh/profiles/<profile>/cordis.patch.yml" }
    ],
    ...overrides
  };
}

async function renderSection(initial: McpAssistantsState, onUpdateSettings = vi.fn()) {
  fixture.getState.mockResolvedValue(initial);
  render(<AiAssistantsSection settings={{ enabled: true }} isSaving={false} accountTabs={tabs} onUpdateSettings={onUpdateSettings} />);
  await waitFor(() => expect(screen.queryByText("claude-code") ?? screen.queryByText(/No assistant/)).toBeTruthy());
  return onUpdateSettings;
}

beforeEach(() => {
  fixture.setPermission.mockResolvedValue(undefined);
  fixture.remove.mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

test("the switch turns the bridge off through an app-scope settings patch", async () => {
  const onUpdateSettings = await renderSection(state());
  fireEvent.click(screen.getByRole("switch", { name: "Let AI assistants connect" }));
  expect(onUpdateSettings).toHaveBeenCalledWith({ scope: "app", mcp: { enabled: false } });
});

test("permission switches change exactly one permission for one account", async () => {
  await renderSection(state());
  const send = screen.getByRole("switch", { name: "Send for Synthetic Workspace" });
  expect(send.getAttribute("aria-checked")).toBe("false");
  expect(screen.getByText("Denied")).toBeTruthy();
  fireEvent.click(send);
  expect(fixture.setPermission).toHaveBeenCalledWith("claude-code", WORK, "send", true);
  // Switches stay disabled while a change is saving.
  const read = screen.getByRole("switch", { name: "Read for Synthetic Workspace" }) as HTMLButtonElement;
  expect(read.disabled).toBe(true);
  await waitFor(() => expect(read.disabled).toBe(false));
  fireEvent.click(read);
  expect(fixture.setPermission).toHaveBeenCalledWith("claude-code", WORK, "read", false);
});

test("accounts the assistant has not used yet are listed as not requested", async () => {
  await renderSection(state());
  const card = screen.getByRole("article", { name: "claude-code" });
  expect(within(card).getByText("Member Home")).toBeTruthy();
  expect(within(card).getByText(/Koushi will ask when claude-code first uses this account/)).toBeTruthy();
});

test("Remove access removes the whole assistant", async () => {
  await renderSection(state());
  fireEvent.click(screen.getByRole("button", { name: "Remove access for claude-code" }));
  expect(fixture.remove).toHaveBeenCalledWith("claude-code");
});

test("registration snippets switch per client and copy their exact text", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  await renderSection(state());
  expect(screen.getByText(/Run this in a terminal/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(
    "claude mcp add --scope user koushi -- '/opt/koushi/koushi-mcp' --client-name claude-code"
  ));

  fireEvent.click(screen.getByRole("tab", { name: "DeepSeek Harness" }));
  expect(screen.getByText(/cordis\.patch\.yml/)).toBeTruthy();
});

test("problems are explained: launcher missing, unreadable store, start failure", async () => {
  await renderSection(state({
    bridge: "start_failed",
    store: "malformed",
    launcher: { state: "unavailable", reason: "not_bundled" },
    registrations: [],
    assistants: []
  }));
  expect(screen.getByText(/could not start the assistant connection/)).toBeTruthy();
  expect(screen.getByText(/cannot read its assistant permissions file/)).toBeTruthy();
  expect(screen.getByText(/does not include the koushi-mcp launcher/)).toBeTruthy();
  expect(screen.queryByRole("tablist")).toBeNull();
});

test("live updates replace the shown state", async () => {
  await renderSection(state());
  act(() => fixture.listeners.forEach(listener => listener(state({ assistants: [] }))));
  expect(screen.getByText("No assistant has asked for access yet.")).toBeTruthy();
});
