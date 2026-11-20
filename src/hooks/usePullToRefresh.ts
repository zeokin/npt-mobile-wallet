import { useEffect, useRef, useState } from "react";

const TRIGGER_PX = 64;
const MAX_PULL_PX = 96;

/// Pull-to-refresh for a scrollable element: attach `ref` to it and render
/// an indicator with `pull` (px) / `refreshing`.
export function usePullToRefresh(onRefresh: () => Promise<unknown>) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [pull, setPull] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const onRefreshRef = useRef(onRefresh);
  onRefreshRef.current = onRefresh;
  const refreshingRef = useRef(false);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let startY: number | null = null;
    let distance = 0;

    const onStart = (e: TouchEvent) => {
      if (el.scrollTop <= 0 && !refreshingRef.current) {
        startY = e.touches[0].clientY;
        distance = 0;
      }
    };
    const onMove = (e: TouchEvent) => {
      if (startY === null) return;
      const dy = e.touches[0].clientY - startY;
      distance = dy > 0 ? Math.min(dy * 0.5, MAX_PULL_PX) : 0;
      setPull(distance);
    };
    const onEnd = async () => {
      if (startY === null) return;
      startY = null;
      if (distance >= TRIGGER_PX) {
        refreshingRef.current = true;
        setRefreshing(true);
        setPull(TRIGGER_PX * 0.75);
        try {
          await onRefreshRef.current();
        } catch {
          /* the caller reports errors */
        } finally {
          refreshingRef.current = false;
          setRefreshing(false);
          setPull(0);
        }
      } else {
        setPull(0);
      }
      distance = 0;
    };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: true });
    el.addEventListener("touchend", onEnd);
    el.addEventListener("touchcancel", onEnd);
    return () => {
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
      el.removeEventListener("touchcancel", onEnd);
    };
  }, []);

  return { ref, pull, refreshing, ready: pull >= TRIGGER_PX };
}
