/** Synchronous focus-scoped bridge between native Edit and DOM-owned history. */
export type EditorHistoryAction = "undo" | "redo";
export interface EditorHistoryAvailability { undo: boolean; redo: boolean }
export interface EditorHistoryRequest {
  action?: EditorHistoryAction;
  availability?: EditorHistoryAvailability;
}
export const EDITOR_HISTORY_EVENT = "koushi-editor-history";
export const EDITOR_HISTORY_CHANGED = "koushi-editor-history-changed";
let nativeAcceleratorsOwned = false;

export function setNativeEditorHistoryAcceleratorsOwned(owned: boolean): void {
  nativeAcceleratorsOwned = owned;
}
export function nativeEditorHistoryAcceleratorsOwned(): boolean {
  return nativeAcceleratorsOwned;
}

function request(detail: EditorHistoryRequest, doc: Document): boolean {
  const event = new CustomEvent(EDITOR_HISTORY_EVENT, { detail, cancelable: true });
  return !(doc.activeElement ?? doc).dispatchEvent(event);
}

export function runFocusedEditorHistory(action: EditorHistoryAction, doc = document): boolean {
  if (request({ action }, doc)) return true;
  const target = doc.activeElement;
  if (!(target instanceof HTMLElement) ||
      !(target.matches("input:not([disabled]):not([readonly]), textarea:not([disabled]):not([readonly])") || target.isContentEditable === true)) return false;
  return doc.execCommand(action);
}

export function focusedEditorHistoryAvailability(doc = document): EditorHistoryAvailability {
  const detail: EditorHistoryRequest = {};
  if (request(detail, doc)) return detail.availability ?? { undo: false, redo: false };
  const target = doc.activeElement;
  const editable = target instanceof HTMLElement &&
    (target.matches("input:not([disabled]):not([readonly]), textarea:not([disabled]):not([readonly])") || target.isContentEditable === true);
  return {
    undo: editable && doc.queryCommandEnabled?.("undo") === true,
    redo: editable && doc.queryCommandEnabled?.("redo") === true
  };
}

export function observeEditorHistoryAvailability(
  callback: (availability: EditorHistoryAvailability) => void,
  doc = document
): () => void {
  const update = () => callback(focusedEditorHistoryAvailability(doc));
  const events = ["focusin", "focusout", "input", "compositionstart", "compositionend", EDITOR_HISTORY_CHANGED];
  events.forEach((name) => doc.addEventListener(name, update));
  update();
  return () => events.forEach((name) => doc.removeEventListener(name, update));
}
