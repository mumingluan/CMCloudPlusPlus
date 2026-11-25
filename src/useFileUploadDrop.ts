import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';

export type FileDropEvent =
  | { type: 'enter' | 'drop'; paths: string[]; position: { x: number; y: number } }
  | { type: 'over'; position: { x: number; y: number } }
  | { type: 'leave' };
export type WatchFileDrop = (callback: (event: FileDropEvent) => void) => Promise<() => void>;

export function useFileUploadDrop({
  area,
  enabled,
  scope,
  watch,
  onDrop,
  onError,
}: {
  area: RefObject<HTMLElement | null>;
  enabled: boolean;
  scope: string;
  watch?: WatchFileDrop;
  onDrop: (paths: string[]) => Promise<unknown>;
  onError: (error: unknown) => void;
}) {
  const context = useRef({ enabled, scope, onDrop, onError });
  context.current = { enabled, scope, onDrop, onError };
  const session = useRef<{ scope: string; count: number } | null>(null);
  const pending = useRef(false);
  const [count, setCount] = useState(0);
  useEffect(() => {
    session.current = null;
    setCount(0);
  }, [scope, enabled]);
  useEffect(() => {
    if (!watch) return;
    let disposed = false;
    let unlisten = () => {};
    const inside = (position: { x: number; y: number }) => {
      const bounds = area.current?.getBoundingClientRect();
      if (!bounds) return false;
      const scale = window.devicePixelRatio || 1;
      const x = position.x / scale,
        y = position.y / scale;
      return x >= bounds.left && x < bounds.right && y >= bounds.top && y < bounds.bottom;
    };
    void watch((event) => {
      if (disposed) return;
      const current = context.current;
      if (event.type === 'leave' || !current.enabled || pending.current) {
        session.current = null;
        setCount(0);
        return;
      }
      if (event.type === 'enter')
        session.current = { scope: current.scope, count: event.paths.length };
      const active = session.current;
      const accepts =
        active && active.scope === current.scope && active.count > 0 && inside(event.position);
      if (event.type !== 'drop') {
        setCount(accepts ? active!.count : 0);
        return;
      }
      session.current = null;
      setCount(0);
      if (!accepts || !event.paths.length) return;
      pending.current = true;
      void Promise.resolve()
        .then(() => current.onDrop([...new Set(event.paths)]))
        .catch((error) => {
          if (!disposed) context.current.onError(error);
        })
        .finally(() => {
          pending.current = false;
        });
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => {
        if (!disposed) context.current.onError(error);
      });
    return () => {
      disposed = true;
      session.current = null;
      unlisten();
    };
  }, [watch, area]);
  return count;
}
