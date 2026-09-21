export interface UiLatencyDiagnostics {
  samples: number;
  lastFrameGapMs: number;
  averageFrameGapMs: number;
  maxFrameGapMs: number;
  longFrameCount: number;
}

export function createUiLatencySampler({ longFrameMs = 50 }: { longFrameMs?: number } = {}) {
  let samples = 0;
  let totalFrameGapMs = 0;
  let lastFrameGapMs = 0;
  let maxFrameGapMs = 0;
  let longFrameCount = 0;

  return {
    recordFrame(frameGapMs: number): UiLatencyDiagnostics {
      if (!Number.isFinite(frameGapMs) || frameGapMs < 0) {
        return snapshot();
      }
      samples += 1;
      totalFrameGapMs += frameGapMs;
      lastFrameGapMs = frameGapMs;
      maxFrameGapMs = Math.max(maxFrameGapMs, frameGapMs);
      if (frameGapMs >= longFrameMs) {
        longFrameCount += 1;
      }
      return snapshot();
    },
    snapshot
  };

  function snapshot(): UiLatencyDiagnostics {
    return {
      samples,
      lastFrameGapMs: roundMs(lastFrameGapMs),
      averageFrameGapMs: samples === 0 ? 0 : roundMs(totalFrameGapMs / samples),
      maxFrameGapMs: roundMs(maxFrameGapMs),
      longFrameCount
    };
  }
}

function roundMs(value: number): number {
  return Math.round(value * 10) / 10;
}
