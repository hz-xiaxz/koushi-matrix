import { useEffect, useState } from "react";

/**
 * The current time, refreshed every `intervalMs` while the caller is mounted.
 *
 * A relative label ("5 minutes ago") has to own its clock: nothing else
 * guarantees a render as time passes (#969 removed the once-per-second `App`
 * render that used to refresh such labels by accident).
 */
export function useNowMs(intervalMs: number): number {
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNowMs(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);

  return nowMs;
}
