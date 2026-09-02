/**
 * Always-on capture of uncaught JS errors and unhandled promise rejections,
 * surfaced as coarse, closed kinds in the diagnostic report. Runtime messages,
 * locations, filenames, and stack frames never enter the diagnostics buffer.
 *
 * Distinct from bootErrorCapture.ts, which is QA-window-title gated and records
 * only the error *kind* for boot detection.
 */
export type CapturedJsErrorKind =
  | "aggregate_error"
  | "error"
  | "eval_error"
  | "range_error"
  | "reference_error"
  | "syntax_error"
  | "type_error"
  | "uri_error"
  | "unknown";

export type CapturedJsErrorChannel = "window_error" | "unhandled_rejection";

export interface CapturedJsError {
  kind: CapturedJsErrorKind;
  channel: CapturedJsErrorChannel;
  ageBucket: "<1m" | "1m-5m" | "5m-30m" | "30m+";
  fingerprint: string;
}

const LIMIT = 20;
const captureStartedAtMs = monotonicNow();
let errors: CapturedJsError[] = [];

export function recordJsError(reason: unknown, channel: CapturedJsErrorChannel): void {
  errors.push({
    kind: errorKind(reason),
    channel,
    ageBucket: jsErrorAgeBucket(monotonicNow() - captureStartedAtMs),
    fingerprint: errorFingerprint(reason)
  });
  if (errors.length > LIMIT) {
    errors = errors.slice(-LIMIT);
  }
}

export function getRecentJsErrors(): CapturedJsError[] {
  return [...errors];
}

export function resetJsErrors(): void {
  errors = [];
}

export function installJsErrorCapture(target: Window): () => void {
  const onError = (event: ErrorEvent) => {
    recordJsError(event.error ?? event.message, "window_error");
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    recordJsError(event.reason, "unhandled_rejection");
  };
  target.addEventListener("error", onError);
  target.addEventListener("unhandledrejection", onRejection);
  return () => {
    target.removeEventListener("error", onError);
    target.removeEventListener("unhandledrejection", onRejection);
  };
}

function errorKind(reason: unknown): CapturedJsErrorKind {
  if (reason instanceof AggregateError) return "aggregate_error";
  if (reason instanceof EvalError) return "eval_error";
  if (reason instanceof RangeError) return "range_error";
  if (reason instanceof ReferenceError) return "reference_error";
  if (reason instanceof SyntaxError) return "syntax_error";
  if (reason instanceof TypeError) return "type_error";
  if (reason instanceof URIError) return "uri_error";
  if (reason instanceof Error) return "error";
  return "unknown";
}

function monotonicNow(): number {
  return typeof performance === "undefined" ? 0 : performance.now();
}

export function jsErrorAgeBucket(ageMs: number): CapturedJsError["ageBucket"] {
  if (ageMs < 60_000) return "<1m";
  if (ageMs < 300_000) return "1m-5m";
  if (ageMs < 1_800_000) return "5m-30m";
  return "30m+";
}

function errorFingerprint(reason: unknown): string {
  const messageLength = reason instanceof Error ? reason.message.length : safeString(reason).length;
  const signature = `${errorKind(reason)}\0${errorFrameLabel(reason)}\0${Math.min(
    255,
    Math.floor(messageLength / 16)
  )}`;
  let hash = 0x811c9dc5;
  for (let index = 0; index < signature.length; index += 1) {
    hash ^= signature.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return `f1_${(hash >>> 0).toString(16).padStart(8, "0")}`;
}

function errorFrameLabel(reason: unknown): string {
  if (!(reason instanceof Error)) return "no_frame";
  const frame = reason.stack?.split("\n")[1]?.trim() ?? "";
  return /^at ([A-Za-z0-9_.$<>-]+)/.exec(frame)?.[1]
    ?? /^([A-Za-z0-9_.$<>-]+)@/.exec(frame)?.[1]
    ?? "anonymous";
}

function safeString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return "unprintable";
  }
}
