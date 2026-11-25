// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { OperationDialog } from './App';
import type { CloudFile, FilePage } from './types';
import type { RepositoryProbe, RepositoryView } from './repositoryApi';

const mocked = vi.hoisted(() => ({ list: vi.fn(), mkdir: vi.fn(), relocate: vi.fn() }));
const repoMocked = vi.hoisted(() => ({
  probe: vi.fn(),
  open: vi.fn(),
  list: vi.fn(),
  close: vi.fn(),
  importFromCloud: vi.fn(),
  exportToCloud: vi.fn(),
  relocateBetweenRepositories: vi.fn(),
}));
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
vi.mock('./repositoryApi', () => ({
  repositoryApi: repoMocked,
  repositoryDemo: false,
  repositoryPreviewUrl: (p: string) => p,
  watchRepositoryDirectory: vi.fn(() => Promise.resolve(() => {})),
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

  it('imports cloud files into a plain repository and disables encrypted repositories', async () => {
    const plainDir = directory('plain-folder', '普通库');
    const encDir = directory('enc-folder', '私密库');
    mocked.list.mockResolvedValue({ files: [plainDir, encDir], nextCursor: null });
    const probes: Record<string, RepositoryProbe> = {
      'plain-folder': {
        folderId: 'plain-folder',
        repositoryId: 'a'.repeat(32),
        kind: 'plain',
        state: 'repository',
        saved: false,
      },
      'enc-folder': {
        folderId: 'enc-folder',
        repositoryId: 'b'.repeat(32),
        kind: 'encrypted',
        state: 'repository',
        saved: false,
      },
    };
    repoMocked.open.mockResolvedValue({
      handle: 'h-plain',
      repositoryId: 'a'.repeat(32),
      folderId: 'plain-folder',
      revision: 'rev-1',
      encrypted: false,
      directory: '',
      items: [],
      conflicts: [],
      historical: false,
      saved: false,
    } satisfies RepositoryView);
    repoMocked.list.mockResolvedValue({
      handle: 'h-plain',
      repositoryId: 'a'.repeat(32),
      folderId: 'plain-folder',
      revision: 'rev-1',
      encrypted: false,
      directory: '',
      items: [
        {
          name: '归档目录',
          path: '归档目录',
          kind: 'directory',
          size: 0,
          conflict: false,
          protected: false,
        },
      ],
      conflicts: [],
      historical: false,
      saved: false,
    } satisfies RepositoryView);
    repoMocked.importFromCloud.mockResolvedValue(undefined);
    repoMocked.close.mockResolvedValue(undefined);
    const sourceFile: CloudFile = {
      id: 'f-1',
      name: 'patch.zip',
      path: '/root',
      isDir: false,
      size: 4096,
      modifiedAt: '',
    };
    const done = vi.fn();
    const { unmount } = render(
      <OperationDialog
        prompt={{ kind: 'move', files: [sourceFile] }}
        parent={root}
        root={root}
        cloudRoot={root}
        knownProbes={probes}
        onClose={vi.fn()}
        onDone={done}
        alert={vi.fn()}
      />,
    );
    const encBtn = await screen.findByRole('button', { name: /私密库/ });
    expect(encBtn.hasAttribute('disabled')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: /普通库/ }));
    const subBtn = await screen.findByRole('button', { name: /归档目录/ });
    repoMocked.list.mockResolvedValueOnce({
      handle: 'h-plain',
      repositoryId: 'a'.repeat(32),
      folderId: 'plain-folder',
      revision: 'rev-2',
      encrypted: false,
      directory: '归档目录',
      items: [],
      conflicts: [],
      historical: false,
      saved: false,
    } satisfies RepositoryView);
    fireEvent.click(subBtn);
    await waitFor(() =>
      expect(screen.queryByRole('status')).toBeNull(),
    );
    fireEvent.click(screen.getByRole('button', { name: '确认' }));
    await waitFor(() =>
      expect(repoMocked.importFromCloud).toHaveBeenCalledWith(
        'h-plain',
        'rev-2',
        '归档目录',
        [sourceFile],
        false,
      ),
    );
    expect(done).toHaveBeenCalledOnce();
    unmount();
    expect(repoMocked.close).toHaveBeenCalledWith('h-plain');
  });

  it('exports plain repository items out to a Mobile Cloud directory', async () => {
    const repoRoot: CloudFile = {
      id: `repository:${'a'.repeat(32)}:`,
      name: 'x3client-hotfix',
      path: '',
      isDir: true,
      size: 0,
      modifiedAt: '',
    };
    const sourceRepo: RepositoryView = {
      handle: 'h-src',
      repositoryId: 'a'.repeat(32),
      folderId: 'repo-folder',
      revision: 'rev-src',
      encrypted: false,
      directory: '1.7.1300',
      items: [],
      conflicts: [],
      historical: false,
      saved: false,
    };
    const repoOps = {
      list: vi.fn().mockResolvedValue({ files: [], nextCursor: null }),
      mkdir: vi.fn(),
      rename: vi.fn(),
      remove: vi.fn(),
      relocate: vi.fn(),
    };
    const outFolder = directory('cloud-out', '导出目录');
    mocked.list
      .mockResolvedValueOnce({ files: [outFolder], nextCursor: null })
      .mockResolvedValueOnce({ files: [], nextCursor: null });
    repoMocked.probe.mockResolvedValue([
      { folderId: 'cloud-out', repositoryId: null, kind: null, state: 'ordinary', saved: false },
    ]);
    repoMocked.exportToCloud.mockResolvedValue(undefined);
    const repoItem: CloudFile = {
      id: `repository:${'a'.repeat(32)}:1.7.1300/Total`,
      name: 'Total',
      path: '1.7.1300/Total',
      isDir: true,
      size: 0,
      modifiedAt: '',
    };
    const done = vi.fn();
    render(
      <OperationDialog
        prompt={{ kind: 'copy', files: [repoItem] }}
        parent={repoRoot}
        root={repoRoot}
        operations={repoOps}
        sourceRepository={sourceRepo}
        cloudRoot={root}
        onClose={vi.fn()}
        onDone={done}
        alert={vi.fn()}
      />,
    );
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull());
    fireEvent.click(screen.getByRole('tab', { name: /移动云盘目录/ }));
    const outBtn = await screen.findByRole('button', { name: /导出目录/ });
    fireEvent.click(outBtn);
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull());
    fireEvent.click(screen.getByRole('button', { name: '确认' }));
    await waitFor(() =>
      expect(repoMocked.exportToCloud).toHaveBeenCalledWith(
        'h-src',
        'rev-src',
        ['1.7.1300/Total'],
        outFolder,
        true,
      ),
    );
    expect(done).toHaveBeenCalledOnce();
  });
});
