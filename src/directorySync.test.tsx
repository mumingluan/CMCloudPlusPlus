// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';
import type { DirectoryChange } from './api';
import type { CloudFile, FilePage } from './types';

const mocked = vi.hoisted(() => ({
  list: vi.fn(),
  remove: vi.fn(),
  notify: null as ((change: DirectoryChange) => void) | null,
}));
vi.mock('./api', () => ({
  api: mocked,
  native: false,
  mobile: false,
  android: false,
  errorText: String,
  watchScan: async () => () => {},
  watchTransfers: async () => () => {},
  watchForeground: async () => () => {},
  watchDirectory: async (callback: (change: DirectoryChange) => void) => {
    mocked.notify = callback;
    return () => {};
  },
}));
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 64,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({ index, key: index, start: index * 64 })),
    measure: () => {},
  }),
}));
const file: CloudFile = {
  id: 'old',
  name: '旧资料.txt',
  path: '/',
  isDir: false,
  size: 10,
  modifiedAt: '',
};
const uploaded: CloudFile = { ...file, id: 'new', name: '刚上传.txt' };
vi.mock('./preview', () => ({
  scalePreview: () => ({
    account: { id: 'test', space: 'group', cloudId: 'cloud', rootId: '/', connected: true },
    files: [{ id: 'old', name: '旧资料.txt', path: '/', isDir: false, size: 10, modifiedAt: '' }],
    jobs: [],
  }),
}));
const change = (overrides: Partial<DirectoryChange> = {}): DirectoryChange => ({
  accountId: 'test',
  space: 'group',
  cloudId: 'cloud',
  parentId: '/',
  file: uploaded,
  ...overrides,
});
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  mocked.notify = null;
  localStorage.clear();
  history.replaceState(null, '', '/?preview=scale');
  mocked.list.mockResolvedValue({ files: [file], nextCursor: null });
  mocked.remove.mockResolvedValue({ removedIds: ['old'], errors: [] });
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
const advance = async (ms: number) => {
  await act(async () => vi.advanceTimersByTimeAsync(ms));
};

it('removes successfully deleted entries immediately despite a stale server listing', async () => {
  render(<App />);
  fireEvent.click(screen.getByRole('checkbox', { name: '选择所有已加载文件' }));
  fireEvent.click(screen.getByRole('button', { name: '删除' }));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '确认删除' })));
  expect(mocked.remove).toHaveBeenCalledWith([file]);
  expect(screen.queryByText('旧资料.txt')).toBeNull();
  expect(mocked.list).toHaveBeenCalledTimes(1);
});

it('shows completed uploads and coalesces a batch independently of the visible transfer page', async () => {
  render(<App />);
  await act(async () => {
    for (let i = 0; i < 100; i++) mocked.notify?.(change());
  });
  await advance(150);
  expect(screen.getByText('刚上传.txt')).toBeTruthy();
  expect(mocked.list).not.toHaveBeenCalled();
  await advance(1350);
  expect(mocked.list).toHaveBeenCalledTimes(1);
  // The service may index the upload later; completion metadata keeps the row visible.
  expect(screen.getByText('刚上传.txt')).toBeTruthy();
  expect(screen.getByText('旧资料.txt')).toBeTruthy();
});

it('ignores completion events for another account, cloud space or directory', async () => {
  render(<App />);
  await act(async () => {
    for (const overrides of [
      { accountId: 'other' },
      { space: 'personal_new' },
      { cloudId: 'other' },
      { parentId: 'other' },
    ])
      mocked.notify?.(change(overrides));
  });
  await advance(2000);
  expect(screen.queryByText('刚上传.txt')).toBeNull();
  expect(mocked.list).not.toHaveBeenCalled();
});

it('invalidates a directory request started before deletion', async () => {
  let resolve!: (value: FilePage) => void;
  mocked.list.mockReturnValueOnce(
    new Promise<FilePage>((done) => {
      resolve = done;
    }),
  );
  render(<App />);
  fireEvent.click(screen.getByRole('button', { name: '刷新目录' }));
  fireEvent.click(screen.getByRole('checkbox', { name: '选择所有已加载文件' }));
  fireEvent.click(screen.getByRole('button', { name: '删除' }));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '确认删除' })));
  await act(async () => resolve({ files: [file], nextCursor: null }));
  expect(screen.queryByText('旧资料.txt')).toBeNull();
  expect(mocked.list).toHaveBeenCalledTimes(2);
});

it('keeps a successful deletion applied when another deletion in the batch fails', async () => {
  mocked.remove.mockResolvedValue({ removedIds: ['old'], errors: ['另一项目：没有权限'] });
  render(<App />);
  await act(async () => mocked.notify?.(change()));
  await advance(150);
  fireEvent.click(screen.getByRole('checkbox', { name: '选择所有已加载文件' }));
  fireEvent.click(screen.getByRole('button', { name: '删除' }));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '确认删除' })));
  expect(document.querySelector('.file-table')?.textContent).not.toContain('旧资料.txt');
  expect(screen.getByText(/另一项目：没有权限/)).toBeTruthy();
  expect(screen.getByRole('dialog')).toBeTruthy();
  mocked.remove.mockResolvedValue({ removedIds: ['new'], errors: [] });
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '确认删除' })));
  expect(mocked.remove.mock.calls[1][0]).toEqual([uploaded]);
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('preserves loaded pages and their cursor during automatic upload reconciliation', async () => {
  const later = { ...file, id: 'later', name: '已加载的第二页.txt' };
  mocked.list
    .mockResolvedValueOnce({ files: [file], nextCursor: 'page-2' })
    .mockResolvedValueOnce({ files: [later], nextCursor: 'page-3' })
    .mockResolvedValueOnce({ files: [uploaded, file], nextCursor: 'page-2' })
    .mockResolvedValueOnce({ files: [], nextCursor: null });
  render(<App />);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '刷新目录' })));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: /加载更多/ })));
  await act(async () => mocked.notify?.(change()));
  await advance(1500);
  expect(screen.getByText('已加载的第二页.txt')).toBeTruthy();
  expect(screen.getByText('刚上传.txt')).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole('button', { name: /加载更多/ })));
  expect(mocked.list.mock.calls.map((args) => args[1])).toEqual([null, 'page-2', null, 'page-3']);
});

it('waits for an in-flight listing instead of restarting it for each upload in a slow batch', async () => {
  let resolve!: (value: FilePage) => void;
  mocked.list.mockReturnValueOnce(
    new Promise<FilePage>((done) => {
      resolve = done;
    }),
  );
  render(<App />);
  fireEvent.click(screen.getByRole('button', { name: '刷新目录' }));
  await act(async () => mocked.notify?.(change()));
  await advance(4500);
  expect(mocked.list).toHaveBeenCalledTimes(1);
  expect(screen.getByText('刚上传.txt')).toBeTruthy();
  await act(async () => resolve({ files: [file], nextCursor: null }));
  await advance(1500);
  expect(mocked.list).toHaveBeenCalledTimes(2);
});
