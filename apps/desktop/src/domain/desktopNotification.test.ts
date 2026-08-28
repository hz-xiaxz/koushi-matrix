import { beforeEach, describe, expect, test, vi } from "vitest";

import {
  clearDesktopAttentionNotifications,
  desktopAttentionNotificationContent,
  sendDesktopAttentionNotification
} from "./desktopNotification";

describe("desktop notification content", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test("builds a redacted payload from allowed attention fields only", () => {
    const payload = desktopAttentionNotificationContent({
      roomDisplayName: "Announcements",
      kind: "mention",
      unreadCount: 6,
      highlightCount: 1
    });

    expect(payload).toEqual({
      title: "Mention in Announcements",
      body: "1 mention, 6 unread"
    });
    expect(Object.keys(payload)).toEqual(["title", "body"]);
    expect(JSON.stringify(payload)).not.toContain("room_id");
    expect(JSON.stringify(payload)).not.toContain("event_id");
    expect(JSON.stringify(payload)).not.toContain("transaction_id");
    expect(JSON.stringify(payload)).not.toContain("sender");
    expect(JSON.stringify(payload)).not.toContain("secret message text");
  });

  test("omits zero-count parts from the body while preserving unread fallback", () => {
    const payload = desktopAttentionNotificationContent({
      roomDisplayName: "Direct chat",
      kind: "dm",
      unreadCount: 1,
      highlightCount: 0
    });

    expect(payload.body).toBe("1 unread");
    expect(payload.body).not.toContain("0 notifications");
    expect(payload.body).not.toContain("0 unread");
  });

  test("sends the redacted payload through a mockable adapter", async () => {
    const transport = {
      notify: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn().mockResolvedValue(undefined)
    };

    await sendDesktopAttentionNotification(
      {
        roomDisplayName: "Direct chat",
        kind: "dm",
        unreadCount: 3,
        highlightCount: 0
      },
      transport
    );

    expect(transport.notify).toHaveBeenCalledOnce();
    expect(transport.notify).toHaveBeenCalledWith({
      title: "Direct message in Direct chat",
      body: "3 unread"
    });
  });

  test("swallows notification transport failures", async () => {
    const transport = {
      notify: vi.fn().mockRejectedValue(new Error("notification failed")),
      clear: vi.fn().mockResolvedValue(undefined)
    };

    const diagnostic = vi.fn();
    await expect(
      sendDesktopAttentionNotification(
        {
          roomDisplayName: "General",
          kind: "message",
          unreadCount: 1,
          highlightCount: 0
        },
        transport,
        diagnostic
      )
    ).resolves.toBeUndefined();
    expect(transport.notify).toHaveBeenCalledOnce();
    expect(diagnostic).toHaveBeenCalledWith("attention_notification_failed");
  });

  test("clears native notifications through a mockable adapter", async () => {
    const transport = {
      notify: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn().mockResolvedValue(undefined)
    };

    await clearDesktopAttentionNotifications(transport);

    expect(transport.clear).toHaveBeenCalledOnce();
  });

  test("reports native notification clear failure with a fixed token", async () => {
    const transport = {
      notify: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn().mockRejectedValue(new Error("private raw failure"))
    };
    const diagnostic = vi.fn();
    await expect(clearDesktopAttentionNotifications(transport, diagnostic)).resolves.toBeUndefined();
    expect(diagnostic).toHaveBeenCalledWith("attention_notification_clear_failed");
    expect(diagnostic).not.toHaveBeenCalledWith(expect.stringContaining("private raw failure"));
  });

  test("swallows native notification clearing failures", async () => {
    const transport = {
      notify: vi.fn().mockResolvedValue(undefined),
      clear: vi.fn().mockRejectedValue(new Error("clear failed"))
    };

    await expect(clearDesktopAttentionNotifications(transport)).resolves.toBeUndefined();
    expect(transport.clear).toHaveBeenCalledOnce();
  });

});
