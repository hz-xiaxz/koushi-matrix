import { existsSync,statSync } from "node:fs";

export function parseQaTitle(title) {
  const status = {};
  for (const token of title.split(/\s+/)) {
    const [key, value] = token.split("=");
    if (!value) {
      continue;
    }
    if (
      ["rooms", "spaces", "timeline_items", "pinned", "pin_ops", "errors", "unread", "badge"].includes(key)
    ) {
      status[key] = Number(value);
    } else if (
      ["active_room", "timeline_room", "timeline_matches_active", "timeline_subscribed"].includes(key)
    ) {
      status[key] = value === "true";
    } else {
      status[key] = value;
    }
  }
  return status;
}


export function qaStatusHasAttentionBaseline(status) {
  return status.unread === 0 && status.badge === 0 && status.notify === "none";
}


function normalizePath(path) {
  return path.replace(/\\/g, "/");
}


export function qaWindowStatePathHasContract(path) {
  return normalizePath(path).endsWith("/app-shell/window-state.json");
}


export function qaStatusHasRequiredPanel(status, requiredPanel) {
  if (status.errors !== 0) {
    return false;
  }
  if (status.panel === requiredPanel) {
    return true;
  }
  return (
    status.panel === "recovery" &&
    (status.session === "needsRecovery" || status.session === "recovering")
  );
}


export function qaStatusHasSendSuccess(status) {
  return status.errors === 0 && status.send === "sent";
}


export function qaStatusIsReady(status, requireRecovered, allowEmptyTimeline = false) {
  const sessionReady = requireRecovered
    ? status.session === "ready"
    : status.session === "ready" || status.session === "needsRecovery";
  const timelineReady = allowEmptyTimeline
    ? Number.isFinite(status.timeline_items) && status.timeline_items >= 0
    : status.timeline_items > 0;
  return (
    sessionReady &&
    status.sync === "running" &&
    status.rooms > 0 &&
    status.active_room === true &&
    status.timeline_room !== false &&
    status.timeline_matches_active !== false &&
    status.timeline_subscribed === true &&
    status.errors === 0 &&
    timelineReady
  );
}



export function requireNonEmptyFile(path, label) {
  if (!existsSync(path) || statSync(path).size === 0) {
    throw new Error(`${label} was not captured`);
  }
}


export function timestamp() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}


export function safeTimestamp() {
  return `${Date.now()}_${process.pid}`.replaceAll("-", "_");
}
