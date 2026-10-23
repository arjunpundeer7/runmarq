import { useEffect, useState } from "react";

/** A shared ticking clock for elapsed times. Ticks only while the window is visible. */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    let id: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (id == null) id = setInterval(() => setNow(Date.now()), intervalMs);
    };
    const stop = () => {
      if (id != null) clearInterval(id);
      id = null;
    };
    const onVis = () => (document.hidden ? stop() : (setNow(Date.now()), start()));
    start();
    document.addEventListener("visibilitychange", onVis);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [intervalMs]);
  return now;
}
