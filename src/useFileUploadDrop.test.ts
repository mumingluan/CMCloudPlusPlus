// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { useFileUploadDrop } from './useFileUploadDrop';
import type { FileDropEvent, WatchFileDrop } from './useFileUploadDrop';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
function setup(enabled = true) {
  vi.stubGlobal('devicePixelRatio', 2);
  const node = document.createElement('section');
  node.getBoundingClientRect = () => ({ left: 100, top: 50, right: 500, bottom: 450 }) as DOMRect;
  const area = { current: node };
  const stop = vi.fn(),
    onDrop = vi.fn(async () => {}),
    onError = vi.fn();
  let callback!: (event: FileDropEvent) => void;
  const watch: WatchFileDrop = vi.fn(async (handler) => {
    callback = handler;
    return stop;
  });
  const hook = renderHook(
    ({ enabled, scope }) => useFileUploadDrop({ area, enabled, scope, watch, onDrop, onError }),
    { initialProps: { enabled, scope: 'folder-a' } },
  );
  return {
    ...hook,
    onDrop,
    onError,
    stop,
    send: (event: FileDropEvent) => act(() => callback(event)),
  };
}
const paths = ['C:/drop/one.txt', 'C:/drop/folder'];
const position = { x: 600, y: 400 };

it('shows the target only inside its bounds and queues mixed sources with physical-pixel scaling', async () => {
  const hook = setup();
  hook.send({ type: 'enter', paths, position });
  expect(hook.result.current).toBe(2);
  hook.send({ type: 'over', position: { x: 50, y: 20 } });
  expect(hook.result.current).toBe(0);
  hook.send({ type: 'over', position });
  expect(hook.result.current).toBe(2);
  hook.send({ type: 'drop', paths: [...paths, paths[0]], position });
  await act(async () => {});
  expect(hook.onDrop).toHaveBeenCalledExactlyOnceWith(paths);
  expect(hook.result.current).toBe(0);
  hook.send({ type: 'enter', paths, position });
  hook.send({ type: 'leave' });
  expect(hook.result.current).toBe(0);
});

it('ignores read-only/disabled targets, outside drops and a drag from a previous directory', async () => {
  const hook = setup(false);
  hook.send({ type: 'enter', paths, position });
  hook.send({ type: 'drop', paths, position });
  hook.rerender({ enabled: true, scope: 'folder-a' });
  hook.send({ type: 'enter', paths, position });
  hook.send({ type: 'drop', paths, position: { x: 1100, y: 900 } });
  hook.send({ type: 'enter', paths, position });
  hook.rerender({ enabled: true, scope: 'folder-b' });
  hook.send({ type: 'drop', paths, position });
  await act(async () => {});
  expect(hook.onDrop).not.toHaveBeenCalled();
});

it('keeps one upload submission pending and removes the native listener on unmount', async () => {
  const hook = setup();
  let finish!: () => void;
  hook.onDrop.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  hook.send({ type: 'enter', paths, position });
  hook.send({ type: 'drop', paths, position });
  await act(async () => {});
  hook.send({ type: 'enter', paths, position });
  hook.send({ type: 'drop', paths, position });
  expect(hook.onDrop).toHaveBeenCalledTimes(1);
  await act(async () => finish());
  hook.unmount();
  expect(hook.stop).toHaveBeenCalledTimes(1);
});
