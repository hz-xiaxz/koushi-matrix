import { useEffect, useMemo } from "react";
// This hook is the React-owned platform-lifecycle seam for desktop attention.
// eslint-disable-next-line no-restricted-imports
import { invoke } from "@tauri-apps/api/core";
// eslint-disable-next-line no-restricted-imports
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  applyDesktopAttentionToWindow,
  createDesktopBadgeSoundDispatcher,
  createTauriDesktopAttentionTransientTransport,
  dispatchDesktopAttentionTransientEffects,
  desktopAttentionNotificationCandidate
} from "../domain/desktopAttention";
import {
  clearDesktopAttentionNotifications,
  createTauriDesktopNotificationTransport,
  sendDesktopAttentionNotification
} from "../domain/desktopNotification";
import type { TimelineDiagnosticLogEntry } from "../components/TimelineView";
import { isTauriRuntime } from "../backend/tauriTimelineTransport";
import type { DesktopAttentionSummary } from "../domain/desktopAttention";
import type { DesktopSnapshot } from "../domain/types";

type DesktopAttentionEffectsInput = {
  snapshot: DesktopSnapshot | null;
  attentionWindowTitle: string;
  safeAttentionSummary: DesktopAttentionSummary;
  appendDiagnosticLog: (entry: TimelineDiagnosticLogEntry) => void;
};

const tauriNotificationTransport = isTauriRuntime()
  ? createTauriDesktopNotificationTransport()
  : null;
const tauriAttentionTransientTransport = isTauriRuntime()
  ? createTauriDesktopAttentionTransientTransport(() =>
        invoke<"played" | "unsupported" | "failed" | "skipped">("play_native_attention_sound")
    )
  : null;
const tauriNativeBadgeTransport = isTauriRuntime()
  ? {
      setBadgeCount: (count?: number) =>
        invoke<"applied" | "unsupported" | "mismatch">("set_native_attention_badge", { count })
    }
  : null;
const desktopBadgeSoundDispatcher = createDesktopBadgeSoundDispatcher();

export function useDesktopAttentionEffects({
  snapshot,
  attentionWindowTitle,
  safeAttentionSummary,
  appendDiagnosticLog
}: DesktopAttentionEffectsInput): void {
  const attentionCapabilities = useMemo(
    () => snapshot?.state.domain.native_attention.summary.capabilities,
    [
      snapshot?.state.domain.native_attention.summary.capabilities.activation,
      snapshot?.state.domain.native_attention.summary.capabilities.badge,
      snapshot?.state.domain.native_attention.summary.capabilities.notifications,
      snapshot?.state.domain.native_attention.summary.capabilities.overlay_icon,
      snapshot?.state.domain.native_attention.summary.capabilities.sound,
      snapshot?.state.domain.native_attention.summary.capabilities.tray
    ]
  );

  useEffect(() => {
    document.title = attentionWindowTitle;
    if (!isTauriRuntime()) {
      return;
    }

    void applyDesktopAttentionToWindow(
      getCurrentWindow(),
      attentionWindowTitle,
      safeAttentionSummary.badgeCount,
      attentionCapabilities,
      (token) => appendDiagnosticLog({
        timestampMs: Date.now(),
        source: "native.attention",
        message: token
      }),
      tauriNativeBadgeTransport ?? undefined
    );

    if (
      !snapshot || snapshot.state.domain.session.kind !== "ready" ||
      !tauriAttentionTransientTransport
    ) {
      desktopBadgeSoundDispatcher.reset();
      return;
    }

    void desktopBadgeSoundDispatcher.observe(
      tauriAttentionTransientTransport,
      safeAttentionSummary.badgeCount,
      snapshot.state.domain.native_attention.summary.capabilities,
      snapshot.state.domain.settings.values.notifications,
      (token) => appendDiagnosticLog({
        timestampMs: Date.now(),
        source: "native.attention",
        message: token
      })
    );
  }, [
    attentionCapabilities,
    attentionWindowTitle,
    safeAttentionSummary.badgeCount,
    snapshot?.state.domain.session.kind,
    snapshot?.state.domain.settings.values.notifications.sound
  ]);

  useEffect(() => {
    if (!snapshot || snapshot.state.domain.session.kind !== "ready") {
      return;
    }

    const candidate = desktopAttentionNotificationCandidate(
      snapshot.state.domain.native_attention
    );

    if (!candidate || !tauriNotificationTransport) {
      return;
    }

    const currentWindow = getCurrentWindow();
    void dispatchDesktopAttentionTransientEffects(
      {
        requestUserAttention: (requestType) => currentWindow.requestUserAttention(requestType)
      },
      candidate,
      snapshot.state.domain.native_attention.summary.capabilities,
      { sound: false },
      (token) => appendDiagnosticLog({
        timestampMs: Date.now(),
        source: "native.attention",
        message: token
      })
    );
    void sendDesktopAttentionNotification(candidate, tauriNotificationTransport, (token) =>
      appendDiagnosticLog({ timestampMs: Date.now(), source: "native.attention", message: token })
    );
  }, [
    snapshot?.state.domain.native_attention.dispatch.kind,
    snapshot?.state.domain.native_attention.summary.candidate?.room_display_name,
    snapshot?.state.domain.native_attention.summary.candidate?.kind,
    snapshot?.state.domain.native_attention.summary.candidate?.unread_count,
    snapshot?.state.domain.native_attention.summary.candidate?.highlight_count
  ]);

  useEffect(() => {
    if (!tauriNotificationTransport || safeAttentionSummary.badgeCount !== 0) {
      return;
    }

    void clearDesktopAttentionNotifications(tauriNotificationTransport, (token) =>
      appendDiagnosticLog({ timestampMs: Date.now(), source: "native.attention", message: token })
    );
  }, [safeAttentionSummary.badgeCount]);
}
