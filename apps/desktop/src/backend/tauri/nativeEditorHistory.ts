import { invoke } from "@tauri-apps/api/core";
import { observeEditorHistoryAvailability, setNativeEditorHistoryAcceleratorsOwned, type EditorHistoryAvailability } from "../../domain/editorHistory";

/** Native menu owns Z accelerators; serialize/coalesce availability writes. */
export function startNativeEditorHistoryBridge(): () => void {
  let closed = false;
  let pending: EditorHistoryAvailability | null = null;
  let running = false;
  async function flush() {
    if (running) return;
    running = true;
    try {
      while (!closed && pending) {
        const value = pending;
        pending = null;
        try {
          const native = await invoke<boolean>("set_native_editor_history_availability", { ...value });
          if (!closed) setNativeEditorHistoryAcceleratorsOwned(native === true);
        } catch {
          if (!closed) setNativeEditorHistoryAcceleratorsOwned(false);
        }
      }
    } catch {
      // Platform availability failure does not mutate editor history.
    } finally { running = false; }
  }
  setNativeEditorHistoryAcceleratorsOwned(false);
  const stop = observeEditorHistoryAvailability(value => { pending = value; void flush(); });
  return () => {
    closed = true;
    pending = null;
    stop();
    setNativeEditorHistoryAcceleratorsOwned(false);
  };
}
