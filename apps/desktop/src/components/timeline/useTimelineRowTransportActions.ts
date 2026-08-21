import { useCallback } from "react";

import type { DiagnosticLogEntry } from "../../domain/diagnostics";
import type { ComposerDocument } from "../../domain/types";
import type { TimelineRowActionHandlers } from "./TimelineItemRow";
import type { TimelineTransport } from "./TimelineTransport";
import { writeClipboardText } from "./TimelineMessageBody";

type TimelineRowTransportActions = Pick<
  TimelineRowActionHandlers,
  | "onSendReaction"
  | "onRetrySend"
  | "onCancelSend"
  | "onRedactReaction"
  | "onEdit"
  | "onRedact"
  | "onPin"
  | "onUnpin"
  | "onDownloadMedia"
  | "onLoadMessageSource"
  | "onForwardMessage"
  | "onLoadLinkPreviews"
  | "onHideLinkPreview"
  | "onCopyText"
>;

export function useTimelineRowTransportActions(
  transport: TimelineTransport,
  timelineDiagnosticKind: string,
  onDiagnosticLogEntry?: (entry: DiagnosticLogEntry) => void
): TimelineRowTransportActions {
  const onSendReaction = useCallback(
    (targetRoomId: string, eventId: string, reactionKey: string) => {
      void transport.sendReaction(targetRoomId, eventId, reactionKey).catch(() => undefined);
    },
    [transport]
  );
  const onRetrySend = useCallback(
    (targetRoomId: string, transactionId: string) => {
      void transport.retrySend(targetRoomId, transactionId).catch(() => undefined);
    },
    [transport]
  );
  const onCancelSend = useCallback(
    (targetRoomId: string, transactionId: string) => {
      void transport.cancelSend(targetRoomId, transactionId).catch(() => undefined);
    },
    [transport]
  );
  const onRedactReaction = useCallback(
    (targetRoomId: string, eventId: string, reactionKey: string, reactionEventId: string) => {
      void transport
        .redactReaction(targetRoomId, eventId, reactionKey, reactionEventId)
        .catch(() => undefined);
    },
    [transport]
  );
  const onEdit = useCallback(
    (targetRoomId: string, eventId: string, document: ComposerDocument) => {
      void transport.editMessage(targetRoomId, eventId, document).catch(() => undefined);
    },
    [transport]
  );
  const onRedact = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.redactMessage(targetRoomId, eventId).catch(() => undefined);
    },
    [transport]
  );
  const onPin = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.pinEvent(targetRoomId, eventId).catch(() => undefined);
    },
    [transport]
  );
  const onUnpin = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.unpinEvent(targetRoomId, eventId).catch(() => undefined);
    },
    [transport]
  );
  const onDownloadMedia = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.downloadMedia(targetRoomId, eventId).catch(() => undefined);
    },
    [transport]
  );
  const onLoadMessageSource = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.loadMessageSource(targetRoomId, eventId).catch(() => undefined);
    },
    [transport]
  );
  const onForwardMessage = useCallback(
    (targetRoomId: string, sourceEventId: string, destinationRoomId: string) => {
      void transport
        .forwardMessage(targetRoomId, sourceEventId, destinationRoomId)
        .catch(() => undefined);
    },
    [transport]
  );
  const onLoadLinkPreviews = useCallback(
    (targetRoomId: string, eventId: string, pendingCount = 0) => {
      onDiagnosticLogEntry?.({
        timestampMs: Date.now(),
        source: "timeline.preview",
        message: `kind=${timelineDiagnosticKind} stage=request trigger=viewport_pending pending=${pendingCount}`
      });
      void transport.loadLinkPreviews?.(targetRoomId, eventId)?.catch(() => {
        onDiagnosticLogEntry?.({
          timestampMs: Date.now(),
          source: "timeline.preview",
          message: `kind=${timelineDiagnosticKind} stage=failed trigger=viewport_pending`
        });
      });
    },
    [onDiagnosticLogEntry, timelineDiagnosticKind, transport]
  );
  const onHideLinkPreview = useCallback(
    (targetRoomId: string, eventId: string) => {
      void transport.hideLinkPreview?.(targetRoomId, eventId)?.catch(() => undefined);
    },
    [transport]
  );
  const onCopyText = useCallback((value: string) => {
    void writeClipboardText(value).catch(() => undefined);
  }, []);

  return {
    onSendReaction,
    onRetrySend,
    onCancelSend,
    onRedactReaction,
    onEdit,
    onRedact,
    onPin,
    onUnpin,
    onDownloadMedia,
    onLoadMessageSource,
    onForwardMessage,
    onLoadLinkPreviews,
    onHideLinkPreview,
    onCopyText
  };
}
