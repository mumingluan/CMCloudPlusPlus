// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { usePullToRefresh } from './usePullToRefresh';

afterEach(cleanup);
function touch(target: HTMLElement, type: string, x: number, y: number) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'touches', {
    value: type === 'touchend' ? [] : [{ clientX: x, clientY: y }],
  });
  target.dispatchEvent(event);
  return event;
}

it('refreshes only after a sufficiently long downward pull at the top', async () => {
  const area = document.createElement('div');
  const ref = { current: area };
  const refresh = vi.fn(async () => {});
  renderHook(() => usePullToRefresh(ref, true, '/', refresh));
  act(() => {
    touch(area, 'touchstart', 50, 10);
    touch(area, 'touchmove', 50, 60);
    touch(area, 'touchend', 50, 60);
  });
  expect(refresh).not.toHaveBeenCalled();
  let moved!: Event;
  await act(async () => {
    touch(area, 'touchstart', 50, 10);
    moved = touch(area, 'touchmove', 50, 160);
    touch(area, 'touchend', 50, 160);
  });
  expect(moved.defaultPrevented).toBe(true);
  expect(refresh).toHaveBeenCalledTimes(1);
});

it('leaves scrolled lists, horizontal gestures and checkbox selection alone', () => {
  const area = document.createElement('div');
  const label = document.createElement('label');
  label.className = 'checkbox-wrap';
  area.append(label);
  const ref = { current: area },
    refresh = vi.fn(async () => {});
  renderHook(() => usePullToRefresh(ref, true, '/', refresh));
  area.scrollTop = 20;
  act(() => {
    touch(area, 'touchstart', 10, 10);
    touch(area, 'touchmove', 10, 200);
    touch(area, 'touchend', 10, 200);
  });
  area.scrollTop = 0;
  act(() => {
    touch(area, 'touchstart', 10, 10);
    touch(area, 'touchmove', 200, 20);
    touch(area, 'touchend', 200, 20);
  });
  act(() => {
    touch(label, 'touchstart', 10, 10);
    touch(label, 'touchmove', 10, 200);
    touch(label, 'touchend', 10, 200);
  });
  expect(refresh).not.toHaveBeenCalled();
});
