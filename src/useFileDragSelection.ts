import { useEffect, useRef, useState } from 'react';
import type { DragEvent, MouseEvent, PointerEvent, RefObject } from 'react';
import type { CloudFile } from './types';

type Box = { left: number; top: number; width: number; height: number };
type Drag = {
  pointer: number;
  x: number;
  y: number;
  clientX: number;
  clientY: number;
  base: Set<string>;
  active: boolean;
  touch: boolean;
  startIndex: number;
  remove: boolean;
  lastPosition?: string;
};

export function useFileDragSelection({
  scrollRef,
  files,
  columns,
  rowHeight,
  selection,
  onSelection,
  resetKey,
}: {
  scrollRef: RefObject<HTMLDivElement | null>;
  files: CloudFile[];
  columns: number;
  rowHeight: number;
  selection: Set<string>;
  onSelection: (selection: Set<string>) => void;
  resetKey: string;
}) {
  const drag = useRef<Drag | null>(null);
  const frame = useRef(0);
  const suppressClick = useRef(false);
  const [box, setBox] = useState<Box | null>(null);
  const latest = useRef({ files, columns, rowHeight, onSelection });
  latest.current = { files, columns, rowHeight, onSelection };

  const stop = () => {
    if (frame.current) cancelAnimationFrame(frame.current);
    frame.current = 0;
    const current = drag.current;
    drag.current = null;
    if (current && scrollRef.current?.hasPointerCapture(current.pointer)) {
      scrollRef.current.releasePointerCapture(current.pointer);
    }
    setBox(null);
  };
  useEffect(() => {
    stop();
    return () => {
      if (frame.current) cancelAnimationFrame(frame.current);
      frame.current = 0;
      const current = drag.current;
      drag.current = null;
      if (current && scrollRef.current?.hasPointerCapture(current.pointer)) {
        scrollRef.current.releasePointerCapture(current.pointer);
      }
    };
  }, [resetKey, files, columns, rowHeight]);

  const update = (schedule = true) => {
    const current = drag.current;
    const area = scrollRef.current;
    if (!current?.active || !area) return;
    const { files, columns, rowHeight, onSelection } = latest.current;
    const bounds = area.getBoundingClientRect();
    const edge = 32;
    const delta =
      current.clientY < bounds.top + edge
        ? -Math.min(20, (bounds.top + edge - current.clientY) / 2)
        : current.clientY > bounds.bottom - edge
          ? Math.min(20, (current.clientY - bounds.bottom + edge) / 2)
          : 0;
    if (delta && schedule) area.scrollTop += delta;
    const x = Math.max(0, Math.min(area.clientWidth, current.clientX - bounds.left));
    const contentHeight = Math.ceil(files.length / columns) * rowHeight;
    const canvasHeight = Math.max(contentHeight, area.clientHeight);
    const y = Math.max(0, Math.min(canvasHeight, current.clientY - bounds.top + area.scrollTop));
    const cellWidth = area.clientWidth / columns;
    const position = `${x}/${y}`;
    if (position === current.lastPosition) {
      if (schedule) frame.current = requestAnimationFrame(() => update());
      return;
    }
    current.lastPosition = position;
    const next = new Set(current.base);
    if (current.touch) {
      const end = Math.min(
        files.length - 1,
        Math.floor(y / rowHeight) * columns + Math.min(columns - 1, Math.floor(x / cellWidth)),
      );
      for (let i = Math.min(current.startIndex, end); i <= Math.max(current.startIndex, end); i++) {
        if (!files[i]) continue;
        if (current.remove) next.delete(files[i].id);
        else next.add(files[i].id);
      }
    } else {
      const left = Math.min(current.x, x);
      const top = Math.min(current.y, y);
      const right = Math.max(current.x, x);
      const bottom = Math.max(current.y, y);
      const firstRow = Math.floor(top / rowHeight);
      const lastRow = Math.min(
        Math.ceil(files.length / columns) - 1,
        Math.max(firstRow, Math.ceil(bottom / rowHeight) - 1),
      );
      const firstColumn = Math.floor(left / cellWidth);
      const lastColumn = Math.min(
        columns - 1,
        Math.max(firstColumn, Math.ceil(right / cellWidth) - 1),
      );
      for (let row = firstRow; row <= lastRow; row++) {
        for (let col = firstColumn; col <= lastColumn; col++) {
          const file = files[row * columns + col];
          if (file) next.add(file.id);
        }
      }
      setBox({ left, top, width: right - left, height: bottom - top });
    }
    onSelection(next);
    if (schedule) frame.current = requestAnimationFrame(() => update());
  };

  return {
    box,
    handlers: {
      onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
        if (drag.current) stop();
        suppressClick.current = false;
        if (event.button !== 0 || !event.isPrimary || !files.length) return;
        const target = event.target as HTMLElement;
        const checkbox = target.closest('.checkbox-wrap');
        if (target.closest('button') && !target.closest('.file-name')) return;
        // Touch scrolling stays native; start a selection swipe on a checkbox.
        if (event.pointerType === 'touch' && !checkbox) return;
        const area = event.currentTarget;
        const bounds = area.getBoundingClientRect();
        if (event.clientX - bounds.left >= area.clientWidth) return;
        const x = event.clientX - bounds.left;
        const canvasHeight = Math.max(
          Math.ceil(files.length / columns) * rowHeight,
          area.clientHeight,
        );
        const y = Math.max(0, Math.min(canvasHeight, event.clientY - bounds.top + area.scrollTop));
        const startIndex =
          Math.floor(y / rowHeight) * columns +
          Math.min(columns - 1, Math.floor(x / (area.clientWidth / columns)));
        drag.current = {
          pointer: event.pointerId,
          x,
          y,
          clientX: event.clientX,
          clientY: event.clientY,
          base: new Set(
            event.ctrlKey || event.metaKey || event.shiftKey || checkbox ? selection : [],
          ),
          active: false,
          touch: event.pointerType === 'touch' || !!checkbox,
          startIndex,
          remove: !!files[startIndex] && selection.has(files[startIndex].id),
        };
      },
      onPointerMove: (event: PointerEvent<HTMLDivElement>) => {
        const current = drag.current;
        if (!current || current.pointer !== event.pointerId) return;
        if (event.pointerType !== 'touch' && !(event.buttons & 1)) {
          stop();
          return;
        }
        current.clientX = event.clientX;
        current.clientY = event.clientY;
        if (!current.active) {
          const bounds = event.currentTarget.getBoundingClientRect();
          const dy = event.clientY - bounds.top + event.currentTarget.scrollTop - current.y;
          if (Math.hypot(event.clientX - bounds.left - current.x, dy) < 6) return;
          current.active = true;
          suppressClick.current = true;
          event.currentTarget.setPointerCapture(event.pointerId);
          update();
        }
        event.preventDefault();
      },
      onPointerUp: (event: PointerEvent<HTMLDivElement>) => {
        const current = drag.current;
        if (!current || current.pointer !== event.pointerId) return;
        if (current.active) {
          current.clientX = event.clientX;
          current.clientY = event.clientY;
          update(false);
        }
        stop();
      },
      onPointerCancel: stop,
      onLostPointerCapture: stop,
      onClickCapture: (event: MouseEvent<HTMLDivElement>) => {
        if (!suppressClick.current) return;
        event.preventDefault();
        event.stopPropagation();
        suppressClick.current = false;
      },
      onDragStart: (event: DragEvent<HTMLDivElement>) => event.preventDefault(),
    },
  };
}
