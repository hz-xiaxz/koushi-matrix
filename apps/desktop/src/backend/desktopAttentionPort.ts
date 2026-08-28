import type {
  DESKTOP_ATTENTION_REQUEST_TYPE,
  DesktopAttentionTransientLike,
  DesktopNativeBadgeLike,
  DesktopWindowLike
} from "../domain/desktopAttention";
import type { DesktopNotificationTransport } from "../domain/desktopNotification";

export interface DesktopAttentionWindowPort extends DesktopWindowLike {
  requestUserAttention(
    requestType: typeof DESKTOP_ATTENTION_REQUEST_TYPE
  ): Promise<void>;
}

export interface DesktopAttentionPort {
  currentWindow(): DesktopAttentionWindowPort;
  notifications: DesktopNotificationTransport;
  sound: DesktopAttentionTransientLike;
  nativeBadge: DesktopNativeBadgeLike;
}
