// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { TransferCategories } from './TransferCategories';
import type { TransferFilter } from './transferView';

let available = 800;
let resize = () => {};
beforeEach(() => {
  available = 800;
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains('transfer-categories') ? available : 0;
  });
  vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get').mockImplementation(function (
    this: HTMLElement,
  ) {
    return this.classList.contains('transfer-tab-groups') ? 600 : 0;
  });
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function Categories() {
  const [value, onChange] = useState<TransferFilter>('active');
  return <TransferCategories value={value} onChange={onChange} />;
}

it('keeps all six categories inline when there is enough room', () => {
  render(<Categories />);
  expect(screen.getAllByRole('button')).toHaveLength(6);
  expect(screen.queryByRole('combobox')).toBeNull();
});

it('switches to the grouped dropdown when space is insufficient and preserves selection on expansion', () => {
  render(<Categories />);
  fireEvent.click(screen.getByRole('button', { name: '已上传' }));
  act(() => {
    available = 400;
    resize();
  });
  const select = screen.getByRole('combobox', { name: '传输分类' }) as HTMLSelectElement;
  expect(select.value).toBe('upload_completed');
  expect(screen.getAllByRole('option')).toHaveLength(6);
  expect(screen.queryByRole('button', { name: '上传中' })).toBeNull();
  fireEvent.change(select, { target: { value: 'download_active' } });
  act(() => {
    available = 800;
    resize();
  });
  expect(screen.queryByRole('combobox')).toBeNull();
  expect(screen.getByRole('button', { name: '下载中' }).getAttribute('aria-pressed')).toBe('true');
});

it('defers the fit decision until the container becomes measurable', () => {
  available = 0;
  render(<Categories />);
  expect(screen.getAllByRole('button')).toHaveLength(6);
  act(() => {
    available = 300;
    resize();
  });
  expect(screen.getByRole('combobox', { name: '传输分类' })).toBeTruthy();
});
