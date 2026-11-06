// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { useState } from 'react';
import type { PointerEvent } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { CloudFile } from './types';
import { useFileDragSelection } from './useFileDragSelection';

const frames = new Map<number, FrameRequestCallback>();
beforeEach(() => {
  let id = 0;
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.set(++id, callback);
    return id;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
});
afterEach(() => {
  cleanup();
  frames.clear();
  vi.unstubAllGlobals();
});

function browser({ count = 3, columns = 1, rowHeight = 48, initial = [] as string[] } = {}) {
  const files: CloudFile[] = Array.from({ length: count }, (_, i) => ({
    id: String(i),
    name: `${i}.txt`,
    path: `/${i}`,
    size: 1,
    isDir: false,
    modifiedAt: '',
  }));
  const area = document.createElement('div');
  const checkbox = document.createElement('label');
  checkbox.className = 'checkbox-wrap';
  area.append(checkbox);
  Object.defineProperties(area, {
    clientWidth: { value: 450 },
    clientHeight: { value: 500 },
  });
  area.getBoundingClientRect = () => ({ left: 0, top: 0, bottom: 500, right: 450 }) as DOMRect;
  const captured = new Set<number>();
  area.setPointerCapture = (id) => {
    captured.add(id);
  };
  area.hasPointerCapture = (id) => captured.has(id);
  area.releasePointerCapture = (id) => {
    captured.delete(id);
  };
  const scrollRef = { current: area };
  const hook = renderHook(
    ({ resetKey }) => {
      const [selection, onSelection] = useState(new Set(initial));
      return {
        ...useFileDragSelection({
          scrollRef,
          files,
          columns,
          rowHeight,
          selection,
          onSelection,
          resetKey,
        }),
        selection,
      };
    },
    { initialProps: { resetKey: 'directory' } },
  );
  const pointer = (x: number, y: number, extra: Partial<PointerEvent<HTMLDivElement>> = {}) =>
    ({
      button: 0,
      buttons: 1,
      isPrimary: true,
      pointerId: 1,
      pointerType: 'mouse',
      clientX: x,
      clientY: y,
      target: area,
      currentTarget: area,
      preventDefault: vi.fn(),
      ...extra,
    }) as PointerEvent<HTMLDivElement>;
  const start = (x: number, y: number, extra = {}) =>
    act(() => hook.result.current.handlers.onPointerDown(pointer(x, y, extra)));
  const move = (x: number, y: number, extra = {}) =>
    act(() => hook.result.current.handlers.onPointerMove(pointer(x, y, extra)));
  return { ...hook, start, move, pointer, area, checkbox, captured };
}

it('keeps a rectangle entirely in the blank space without selecting the last file', () => {
  const h = browser();
  h.start(60, 220);
  h.move(140, 280);
  expect([...h.result.current.selection]).toEqual([]);
  expect(h.result.current.box).toEqual({ left: 60, top: 220, width: 80, height: 60 });
});

it('selects only intersected files when dragging upward from the blank space', () => {
  const h = browser();
  h.start(60, 220);
  h.move(140, 70);
  expect([...h.result.current.selection]).toEqual(['1', '2']);
  expect(h.result.current.box).toEqual({ left: 60, top: 70, width: 80, height: 150 });
});

it('does not select a file in empty grid slots or just across a cell boundary', () => {
  const h = browser({ count: 4, columns: 3, rowHeight: 152 });
  h.start(160, 180);
  h.move(280, 250);
  expect([...h.result.current.selection]).toEqual([]);
  h.move(150, 40);
  act(() => {
    const [id, callback] = [...frames.entries()][0];
    frames.delete(id);
    callback(0);
  });
  expect([...h.result.current.selection]).toEqual(['1']);
});

it('includes the final pointer location when release happens before the next animation frame', () => {
  const h = browser();
  h.start(60, 12);
  h.move(140, 120);
  expect([...h.result.current.selection]).toEqual(['0', '1', '2']);
  act(() => h.result.current.handlers.onPointerUp(h.pointer(140, 40)));
  expect([...h.result.current.selection]).toEqual(['0']);
  expect(h.result.current.box).toBeNull();
  expect(frames.size).toBe(0);
  expect(h.captured.size).toBe(0);
});

it('preserves existing selection when Ctrl is held while starting in blank space', () => {
  const h = browser({ initial: ['0'] });
  h.start(60, 220, { ctrlKey: true });
  h.move(140, 110);
  expect([...h.result.current.selection]).toEqual(['0', '2']);
});

it('preserves native touch scrolling and supports checkbox swipe deselection', () => {
  const h = browser({ initial: ['0', '1', '2'] });
  h.start(60, 12, { pointerType: 'touch' });
  h.move(60, 120, { pointerType: 'touch' });
  expect(h.result.current.box).toBeNull();
  expect([...h.result.current.selection]).toEqual(['0', '1', '2']);
  h.start(10, 60, { target: h.checkbox, pointerType: 'touch' });
  h.move(10, 120, { pointerType: 'touch' });
  expect([...h.result.current.selection]).toEqual(['0']);
});

it('cancels pending animation frames when the directory changes', () => {
  const h = browser();
  h.start(60, 220);
  h.move(140, 70);
  h.rerender({ resetKey: 'other-directory' });
  expect(frames.size).toBe(0);
  expect(h.result.current.box).toBeNull();
});
