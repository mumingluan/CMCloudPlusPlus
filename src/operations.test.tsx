// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { OperationDialog } from './App';
import type { CloudFile, FilePage } from './types';

const mocked = vi.hoisted(() => ({ list: vi.fn(), mkdir: vi.fn() }));
vi.mock('./api', () => ({
  api: mocked,
  native: false,
  android: false,
  mobile: false,
  errorText: (error: unknown) => String(error),
  watchTransfers: vi.fn(),
  watchScan: vi.fn(),
  watchForeground: vi.fn(),
}));
const directory = (id: string, name: string): CloudFile => ({
  id,
  name,
  path: `/${id}`,
  isDir: true,
  size: 0,
  modifiedAt: '',
});
const root = directory('root', '全部文件');
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
beforeEach(() => {
  vi.clearAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});
afterEach(cleanup);
describe('file operation recovery and request races', () => {
  it('submits only one create while repeated Enter events arrive during a pending request', async () => {
    const pending = deferred<void>();
    mocked.mkdir.mockReturnValue(pending.promise);
    const done = vi.fn();
    render(
      <OperationDialog
        prompt={{ kind: 'mkdir', files: [] }}
        parent={root}
        root={root}
        onClose={vi.fn()}
        onDone={done}
        alert={vi.fn()}
      />,
    );
    const input = screen.getByRole('textbox', { name: '名称' });
    fireEvent.change(input, { target: { value: '家庭资料' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(mocked.mkdir).toHaveBeenCalledTimes(1);
    expect(input.hasAttribute('disabled')).toBe(true);
    await act(async () => pending.resolve());
    expect(done).toHaveBeenCalledTimes(1);
  });
  it('clears old directory entries and ignores a late response after returning to the root', async () => {
    const first = deferred<FilePage>();
    const child = deferred<FilePage>();
    const returned = deferred<FilePage>();
    mocked.list
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(child.promise)
      .mockReturnValueOnce(returned.promise);
    render(
      <OperationDialog
        prompt={{ kind: 'move', files: [directory('source', '待移动目录')] }}
        parent={root}
        root={root}
        onClose={vi.fn()}
        onDone={vi.fn()}
        alert={vi.fn()}
      />,
    );
    await act(async () => first.resolve({ files: [directory('a', '目标 A')], nextCursor: null }));
    fireEvent.click(screen.getByRole('button', { name: '目标 A' }));
    expect(document.querySelector('.destination-list')?.textContent).not.toContain('目标 A');
    fireEvent.click(screen.getByRole('button', { name: '全部文件' }));
    await act(async () =>
      child.resolve({ files: [directory('stale', '过期目录')], nextCursor: null }),
    );
    expect(screen.queryByText('过期目录')).toBeNull();
    expect(screen.getByRole('status').textContent).toContain('读取目标目录');
    await act(async () =>
      returned.resolve({ files: [directory('fresh', '最新目录')], nextCursor: null }),
    );
    await waitFor(() => expect(screen.getByRole('button', { name: '最新目录' })).toBeTruthy());
    expect(screen.queryByText('过期目录')).toBeNull();
    expect(mocked.list).toHaveBeenCalledTimes(3);
  });
});
