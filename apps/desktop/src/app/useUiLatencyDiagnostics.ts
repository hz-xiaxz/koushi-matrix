import { useEffect, useState } from "react";
import {
  createUiLatencySampler,
  EMPTY_UI_LATENCY_DIAGNOSTICS,
  type UiLatencyDiagnostics
} from "../domain/uiLatency";

export function useUiLatencyDiagnostics(): UiLatencyDiagnostics {
  const [diagnostics, setDiagnostics] = useState<UiLatencyDiagnostics>(
    EMPTY_UI_LATENCY_DIAGNOSTICS
  );

  useEffect(() => {
    if (typeof window.requestAnimationFrame !== "function") {
      return;
    }
    const sampler = createUiLatencySampler();
    let frameId = 0;
    let lastFrameAt = 0;
    let lastPublishedAt = 0;
    let cancelled = false;

    const publishIfChanged = (next: UiLatencyDiagnostics) => {
      setDiagnostics((current) =>
        current.samples === next.samples &&
        current.lastFrameGapMs === next.lastFrameGapMs &&
        current.averageFrameGapMs === next.averageFrameGapMs &&
        current.maxFrameGapMs === next.maxFrameGapMs &&
        current.longFrameCount === next.longFrameCount
          ? current
          : next
      );
    };

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
        if (now - lastPublishedAt >= 1000) {
          lastPublishedAt = now;
          publishIfChanged(next);
        }
      }
      frameId = window.requestAnimationFrame(tick);
    };

    frameId = window.requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(frameId);
    };
  }, []);

  return diagnostics;
}
