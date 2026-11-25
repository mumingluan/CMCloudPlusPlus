import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';

export function usePullToRefresh(
  ref: RefObject<HTMLDivElement | null>,
  enabled: boolean,
  scope: string,
  refresh: () => Promise<unknown>,
) {
  const callback = useRef(refresh);
  callback.current = refresh;
  const [distance, setDistance] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const busy = useRef(false);
  useEffect(() => {
    const area = ref.current;
    if (!area || !enabled) return;
    let gesture: { x: number; y: number; pull: number } | null = null;
    let disposed = false;
    const start = (event: TouchEvent) => {
      if (
        busy.current ||
        area.scrollTop > 0 ||
        event.touches.length !== 1 ||
        (event.target as HTMLElement).closest(
          '.checkbox-wrap, button:not(.file-name), input, select, textarea',
        )
      )
        return;
      gesture = { x: event.touches[0].clientX, y: event.touches[0].clientY, pull: 0 };
    };
    const move = (event: TouchEvent) => {
      if (!gesture) return;
      if (event.touches.length !== 1 || area.scrollTop > 0) {
        cancel();
        return;
      }
      const dx = Math.abs(event.touches[0].clientX - gesture.x);
      const dy = event.touches[0].clientY - gesture.y;
      if (dy <= 0 || dx > Math.abs(dy)) {
        cancel();
        return;
      }
      if (dy > 8) {
        event.preventDefault();
        gesture.pull = Math.min(88, dy * 0.45);
        setDistance(gesture.pull);
      }
    };
    const cancel = () => {
      gesture = null;
      setDistance(0);
    };
    const end = () => {
      const trigger = (gesture?.pull ?? 0) >= 56;
      cancel();
      if (!trigger || busy.current) return;
      busy.current = true;
      setRefreshing(true);
      void Promise.resolve()
        .then(() => callback.current())
        .catch(() => {})
        .finally(() => {
          busy.current = false;
          if (!disposed) setRefreshing(false);
        });
    };
    area.addEventListener('touchstart', start, { passive: true });
    area.addEventListener('touchmove', move, { passive: false });
    area.addEventListener('touchend', end);
    area.addEventListener('touchcancel', cancel);
    return () => {
      disposed = true;
      cancel();
      setRefreshing(false);
      area.removeEventListener('touchstart', start);
      area.removeEventListener('touchmove', move);
      area.removeEventListener('touchend', end);
      area.removeEventListener('touchcancel', cancel);
    };
  }, [ref, enabled, scope]);
  return { distance, refreshing };
}
