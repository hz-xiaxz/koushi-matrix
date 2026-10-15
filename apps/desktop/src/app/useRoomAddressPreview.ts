import { useEffect, useState } from "react";
import type { DesktopApi } from "../backend/desktopApi";
import type { RoomAddressPreview } from "../domain/types";

/**
 * Render a Rust projection of the current unsent address, never an older
 * response. `alias === null` is the create session's random default (#1416),
 * which exists only once Rust has opened that session's draft, so `draftReady`
 * is part of the identity.
 */
export function useRoomAddressPreview(
  api: Pick<DesktopApi, "previewRoomAddress">,
  sessionId: number,
  alias: string | null,
  account: string | null,
  draftReady: boolean,
  enabled: boolean
): RoomAddressPreview | null {
  const key = `${account ?? ""}\u0000${sessionId}\u0000${draftReady ? 1 : 0}\u0000${alias ?? "\u0001"}`;
  const [result, setResult] = useState<{ key: string; preview: RoomAddressPreview } | null>(null);
  useEffect(() => {
    let current = true;
    if (enabled) {
      void api.previewRoomAddress(sessionId, alias).then(preview => {
        if (current) setResult({ key, preview });
      }).catch(() => {
        if (current) setResult(null);
      });
    }
    return () => { current = false; };
  }, [api, sessionId, alias, key, enabled]);
  return enabled && result?.key === key ? result.preview : null;
}
