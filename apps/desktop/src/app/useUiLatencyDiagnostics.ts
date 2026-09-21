import { useCallback, useEffect, useRef, useState } from "react";
import { createUiLatencySampler, type UiLatencyDiagnostics } from "../domain/uiLatency";

/**
 * Samples animation-frame gaps for the diagnostic report and returns a stable
 * reader for the current totals.
 *
 * Issue #969: the samples stay out of React state. This hook lives in `App`,
 * so publishing a sample re-renders the whole tree, and the totals are only
 * needed when a report is built. `live` opts into that periodic render while
 * the Diagnostics dialog is open, so the report on screen keeps moving.
 */
export function useUiLatencyDiagnostics({ live }: { live: boolean }): () => UiLatencyDiagnostics {
  const [sampler] = useState(() => createUiLatencySampler());
  const [, setPublishedSamples] = useState(0);
  const liveRef = useRef(live);

  useEffect(() => {
    liveRef.current = live;
  }, [live]);

  useEffect(() => {
    if (typeof window.requestAnimationFrame !== "function") {
      return;
    }
    let frameId = 0;
    let lastFrameAt = 0;
    let lastPublishedAt = 0;
    let cancelled = false;

    const tick = (now: number) => {
      if (cancelled) {
        return;
      }
      if (lastFrameAt === 0) {
        lastFrameAt = now;
        lastPublishedAt = now;
      } else {
        const next = sampler.recordFrame(now - lastFrameAt);
        lastFrameAt = now;
        if (liveRef.current && now - lastPublishedAt >= 1000) {
          lastPublishedAt = now;
          setPublishedSamples(next.samples);
        }
      }
      frameId = window.requestAnimationFrame(tick);
    };

    frameId = window.requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(frameId);
    };
  }, [sampler]);

  return useCallback(() => sampler.snapshot(), [sampler]);
}
