// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import type { PointerEvent } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useMobileFileLongPress } from './useMobileFileLongPress';

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
const pointer = (x = 10, y = 10, target: HTMLElement = document.createElement('div')) =>
  ({
    pointerId: 1,
    pointerType: 'touch',
    clientX: x,
    clientY: y,
    target,
  }) as unknown as PointerEvent<HTMLDivElement>;

it('selects on a held touch and consumes the release click so the file stays selected', () => {
  const select = vi.fn();
  const { result } = renderHook(() => useMobileFileLongPress(true, select, '/'));
  result.current.start('file', pointer());
  act(() => vi.advanceTimersByTime(450));
  expect(select).toHaveBeenCalledWith('file');
  result.current.cancel();
  expect(result.current.consumeClick('file')).toBe(true);
  expect(result.current.consumeClick('file')).toBe(false);
});

it('keeps taps, scrolling, cancelled touches and directory navigation from selecting files', () => {
  const select = vi.fn();
  const { result, rerender } = renderHook(
    ({ scope }) => useMobileFileLongPress(true, select, scope),
    { initialProps: { scope: '/' } },
  );
  result.current.start('tap', pointer());
  result.current.cancel();
  act(() => vi.advanceTimersByTime(500));
  result.current.start('scroll', pointer());
  result.current.move(pointer(10, 35));
  act(() => vi.advanceTimersByTime(500));
  result.current.start('old-folder', pointer());
  rerender({ scope: '/other' });
  act(() => vi.advanceTimersByTime(500));
  expect(select).not.toHaveBeenCalled();
});

it('leaves checkbox swipe selection and mouse input to their existing handlers', () => {
  const select = vi.fn();
  const { result } = renderHook(() => useMobileFileLongPress(true, select, '/'));
  const label = document.createElement('label');
  label.className = 'checkbox-wrap';
  result.current.start('checkbox', pointer(10, 10, label));
  act(() => vi.advanceTimersByTime(500));
  result.current.start('mouse', {
    ...pointer(),
    pointerType: 'mouse',
  } as PointerEvent<HTMLDivElement>);
  act(() => vi.advanceTimersByTime(500));
  expect(select).not.toHaveBeenCalled();
});
