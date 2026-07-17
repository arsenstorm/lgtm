import { useEffect, useRef } from "react";

const MIN_INTERVAL_MS = 5000;

/**
 * Re-runs `refresh` whenever the window regains focus, throttled so rapid
 * focus flips don't hammer git or the GitHub API.
 */
export function useAutoRefresh(refresh: () => void) {
  const lastRunRef = useRef(0);
  useEffect(() => {
    const onFocus = () => {
      const now = Date.now();
      if (now - lastRunRef.current < MIN_INTERVAL_MS) {
        return;
      }
      lastRunRef.current = now;
      refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);
}
