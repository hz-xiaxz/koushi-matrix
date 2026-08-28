import type {
  DesktopAttentionDiagnosticSink,
  DesktopAttentionNotificationCandidate
} from "./desktopAttention";

export interface DesktopNotificationContent {
  title: string;
  body: string;
}

export interface DesktopNotificationTransport {
  notify(content: DesktopNotificationContent): Promise<void>;
  clear(): Promise<void>;
}

export function desktopAttentionNotificationContent(
  candidate: DesktopAttentionNotificationCandidate
): DesktopNotificationContent {
  switch (candidate.kind) {
    case "mention":
      return {
        title: `Mention in ${candidate.roomDisplayName}`,
        body: joinAttentionCounts([
          formatCount(candidate.highlightCount, "mention"),
          formatCount(candidate.unreadCount, "unread", "unread")
        ])
      };
    case "dm":
      return {
        title: `Direct message in ${candidate.roomDisplayName}`,
        body: joinAttentionCounts([formatCount(candidate.unreadCount, "unread", "unread")])
      };
    case "message":
      return {
        title: `Message in ${candidate.roomDisplayName}`,
        body: joinAttentionCounts([formatCount(candidate.unreadCount, "unread", "unread")])
      };
  }
}

export async function sendDesktopAttentionNotification(
  candidate: DesktopAttentionNotificationCandidate,
  transport: DesktopNotificationTransport,
  diagnostic?: DesktopAttentionDiagnosticSink
): Promise<void> {
  try {
    await transport.notify(desktopAttentionNotificationContent(candidate));
  } catch {
    diagnostic?.("attention_notification_failed");
  }
}

export async function clearDesktopAttentionNotifications(
  transport: DesktopNotificationTransport,
  diagnostic?: DesktopAttentionDiagnosticSink
): Promise<void> {
  try {
    await transport.clear();
  } catch {
    diagnostic?.("attention_notification_clear_failed");
  }
}

function joinAttentionCounts(parts: string[]): string {
  return parts.filter((part) => part.length > 0).join(", ");
}

function formatCount(count: number, singularLabel: string, pluralLabel = `${singularLabel}s`): string {
  if (count === 0) {
    return "";
  }
  return `${count} ${count === 1 ? singularLabel : pluralLabel}`;
}
