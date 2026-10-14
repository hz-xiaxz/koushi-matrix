import { useEffect, useRef, useState } from "react";

import type { DesktopApi } from "../backend/desktopApi";
import type {
  CreateRoomAccessPreview,
  CreateRoomAccessPreviewInput,
  RoomAccessDraftScope,
  RoomAccessPreview,
  RoomAccessPreviewContext
} from "../domain/types";

/**
 * The selection identity of one preview: scope, context, the caller's
 * selection key (the Rust draft revision plus any confirmed facts it shows)
 * and the resolved inputs. The published `stateGeneration` is deliberately
 * not part of it (#1417).
 */
function previewIdentity(
  scope: RoomAccessDraftScope | null,
  context: string,
  selectionKey: string,
  extra = ""
): string {
  return `${scope ? JSON.stringify(scope) : ""}|${context}|${selectionKey}|${extra}`;
}

/**
 * Stale-while-revalidate over one Rust preview (#1177, #1417).
 *
 * Every published `stateGeneration` re-requests the preview, but only a change
 * of the selection identity hides the shown result: an unrelated snapshot
 * advance (sync, typing, another room) keeps the current details on screen
 * until the newer response replaces them, so the panel never collapses,
 * flickers or moves the scroll position. A response is admitted only when it
 * is newer than the last admitted one and was requested for the identity that
 * is still current, so an older response never replaces newer details and a
 * different selection's preview is never rendered.
 */
function useRevalidatedPreview<T>(
  request: () => Promise<T>,
  identity: string,
  stateGeneration: number | undefined,
  active: boolean
): T | null {
  const [result, setResult] = useState<{ identity: string; preview: T } | null>(null);
  const currentIdentityRef = useRef(identity);
  currentIdentityRef.current = identity;
  const requestSeqRef = useRef(0);
  const admittedSeqRef = useRef(0);
  const requestRef = useRef(request);
  requestRef.current = request;
  useEffect(() => {
    if (!active) {
      return;
    }
    requestSeqRef.current += 1;
    const seq = requestSeqRef.current;
    const requestIdentity = identity;
    const admit = (next: { identity: string; preview: T } | null) => {
      if (seq <= admittedSeqRef.current || currentIdentityRef.current !== requestIdentity) {
        return;
      }
      admittedSeqRef.current = seq;
      setResult(next);
    };
    void requestRef
      .current()
      .then((preview) => admit({ identity: requestIdentity, preview }))
      .catch(() => admit(null));
  }, [identity, stateGeneration, active]);
  return active && result?.identity === identity ? result.preview : null;
}

/**
 * Issue #1177: render the Rust access/history preview for one Room Info panel,
 * never an older response and never a result for a different selection.
 */
export function useRoomAccessPreview(
  api: Pick<DesktopApi, "previewRoomAccess">,
  scope: RoomAccessDraftScope | null,
  context: RoomAccessPreviewContext,
  selectionKey: string,
  stateGeneration: number | undefined,
  enabled: boolean
): RoomAccessPreview | null {
  const identity = previewIdentity(scope, context, selectionKey);
  return useRevalidatedPreview(
    () => (scope ? api.previewRoomAccess(scope, context) : Promise.reject(new Error("no scope"))),
    identity,
    stateGeneration,
    enabled && scope !== null
  );
}

/**
 * Issue #1177: render the Rust effective-proposed-tuple preview for the create
 * dialog, fenced the same way. The input is a plain value object, so it is
 * keyed by value.
 */
export function useCreateRoomAccessPreview(
  api: Pick<DesktopApi, "previewCreateRoomAccess">,
  scope: RoomAccessDraftScope | null,
  input: CreateRoomAccessPreviewInput,
  selectionKey: string,
  stateGeneration: number | undefined,
  enabled: boolean
): CreateRoomAccessPreview | null {
  const inputKey = JSON.stringify(input);
  const identity = previewIdentity(scope, "create", selectionKey, inputKey);
  return useRevalidatedPreview(
    () =>
      scope
        ? api.previewCreateRoomAccess(scope, JSON.parse(inputKey) as CreateRoomAccessPreviewInput)
        : Promise.reject(new Error("no scope")),
    identity,
    stateGeneration,
    enabled && scope !== null
  );
}
