import { useEffect, useRef } from 'react';
import type { PointerEvent } from 'react';

export function useMobileFileLongPress(
  enabled: boolean,
  select: (id: string) => void,
  scope: string,
) {
  const callback = useRef(select);
  callback.current = select;
  const gesture = useRef<{ id: string; pointer: number; x: number; y: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const suppressed = useRef<string | null>(null);
  const cancel = () => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    gesture.current = null;
  };
  useEffect(() => cancel, [enabled, scope]);

  return {
    start(id: string, event: PointerEvent<HTMLDivElement>) {
      cancel();
      suppressed.current = null;
      if (!enabled || !['touch', 'pen'].includes(event.pointerType)) return;
      if ((event.target as HTMLElement).closest('.checkbox-wrap, .row-more')) return;
      gesture.current = { id, pointer: event.pointerId, x: event.clientX, y: event.clientY };
      timer.current = setTimeout(() => {
        if (gesture.current?.id !== id) return;
        suppressed.current = id;
        callback.current(id);
        timer.current = null;
      }, 450);
    },
    move(event: PointerEvent<HTMLDivElement>) {
      const current = gesture.current;
      if (
        current &&
        (event.pointerId !== current.pointer ||
          Math.hypot(event.clientX - current.x, event.clientY - current.y) > 10)
      )
        cancel();
    },
    cancel,
    context(id: string) {
      cancel();
      suppressed.current = id;
      callback.current(id);
    },
    consumeClick(id: string) {
      if (suppressed.current !== id) return false;
      suppressed.current = null;
      return true;
    },
  };
}
