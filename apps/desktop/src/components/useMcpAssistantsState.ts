import { useEffect, useState } from "react";
import { api } from "../backend/appRuntime";
import { desktopEventPort } from "../backend/desktopEventRuntime";
import { isTauriRuntime } from "../backend/runtimeEnvironment";
import type { AccountTabSummary, McpAssistantsState } from "../domain/types";
import { accountTabLabel } from "./accountSignOut";

/**
 * Follows the Rust consent broker's process-wide state (#1536): the pushed
 * `koushi-desktop://mcp` events, plus one initial read. A live event observed
 * while the initial read is in flight wins, as for desktop updates.
 */
export function useMcpAssistantsState(): McpAssistantsState | null {
  const [state, setState] = useState<McpAssistantsState | null>(null);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let revision = 0;
    let unlisten: (() => void) | null = null;
    void desktopEventPort
      .listenMcpAssistants(next => {
        revision++;
        if (!disposed) setState(next);
      })
      .then(async dispose => {
        if (disposed) {
          dispose();
          return;
        }
        unlisten = dispose;
        const requestedRevision = revision;
        const initial = await api.getMcpAssistantsState();
        if (!disposed && revision === requestedRevision) setState(initial);
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  return state;
}

/**
 * The stable id the MCP bridge names a tab's account by: `account:<user id>`.
 * A tab that signed in during this run still has its sign-in tab id, so the
 * tab id alone cannot be matched against the bridge's account ids.
 */
export function mcpAccountId(tab: AccountTabSummary): string | null {
  return tab.accountKey ? `account:${tab.accountKey}` : null;
}

export function mcpAccountTab(
  accountId: string,
  tabs: readonly AccountTabSummary[]
): AccountTabSummary | undefined {
  return tabs.find(candidate => mcpAccountId(candidate) === accountId);
}

/**
 * An account as Koushi itself names it. Assistants only ever supply an account
 * id; the label comes from Koushi's own tabs, never from the request.
 */
export function mcpAccountLabel(
  accountId: string,
  tabs: readonly AccountTabSummary[]
): string | null {
  const tab = mcpAccountTab(accountId, tabs);
  return tab ? accountTabLabel(tab) : null;
}
