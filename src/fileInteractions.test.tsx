// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';
import type { AccountSummary, TransferJob } from './types';
import type { RepositoryView } from './repositoryApi';

const mocked = vi.hoisted(() => ({
  list: vi.fn(),
  mkdir: vi.fn(),
  controlBatch: vi.fn(),
  jobs: vi.fn(),
  previewJobs: [] as TransferJob[],
  link: vi.fn(),
  openUrl: vi.fn(),
  preview: vi.fn(),
  space: 'group' as AccountSummary['space'],
  probe: vi.fn(),
  createNamedRepository: vi.fn(),
  openRepository: vi.fn(),
  listRepository: vi.fn(),
  closeRepository: vi.fn(),
  labelCache: vi.fn(),
  probeCrypt: vi.fn(),
  createCrypt: vi.fn(),
  closeCrypt: vi.fn(),
}));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mocked.openUrl }));
vi.mock('./api', () => ({
  api: mocked,
  native: false,
  mobile: false,
  android: false,
  errorText: String,
  watchScan: async () => () => {},
  watchTransfers: async () => () => {},
  watchForeground: async () => () => {},
  watchDirectory: async () => () => {},
}));
vi.mock('./repositoryApi', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./repositoryApi')>();
  return {
    ...actual,
    repositoryApi: {
      ...actual.repositoryApi,
      probe: mocked.probe,
      createNamed: mocked.createNamedRepository,
      open: mocked.openRepository,
      list: mocked.listRepository,
      close: mocked.closeRepository,
      labelCache: mocked.labelCache,
    },
  };
});
vi.mock('./cryptApi', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./cryptApi')>();
  return {
    ...actual,
    cryptApi: {
      ...actual.cryptApi,
      probe: mocked.probeCrypt,
      create: mocked.createCrypt,
      close: mocked.closeCrypt,
    },
  };
});
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 48,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({ index, key: index, start: index * 48 })),
    measure: () => {},
  }),
}));
vi.mock('./preview', () => ({
  scalePreview: () => ({
    account: {
      id: 'test',
      phone: '19900001111',
      space: mocked.space,
      cloudId: 'cloud',
      rootId: '/',
      connected: true,
    },
    files: [
      { id: 'folder', name: '文件夹', path: '/folder', isDir: true, size: 0, modifiedAt: '' },
      {
        id: 'zip',
        name: '资料.zip',
        path: '/zip',
        isDir: false,
        size: 10,
        modifiedAt: '2024-01-01',
      },
      {
        id: 'image',
        name: '图片.png',
        path: '/image',
        isDir: false,
        size: 20,
        modifiedAt: '2026-01-01',
      },
      {
        id: 'video',
        name: '影片.mp4',
        path: '/video',
        isDir: false,
        size: 30,
        modifiedAt: '2025-01-01',
      },
    ],
    jobs: mocked.previewJobs,
  }),
}));
beforeEach(() => {
  vi.resetAllMocks();
  mocked.space = 'group';
  mocked.previewJobs = [];
  mocked.controlBatch.mockResolvedValue(1);
  mocked.jobs.mockResolvedValue([]);
  localStorage.clear();
  history.replaceState(null, '', '/?preview=scale');
  mocked.list.mockResolvedValue({ files: [], nextCursor: null });
  mocked.mkdir.mockResolvedValue(undefined);
  mocked.closeRepository.mockResolvedValue(undefined);
  mocked.probeCrypt.mockResolvedValue([]);
  mocked.closeCrypt.mockResolvedValue(undefined);
  mocked.link.mockResolvedValue('https://storage.example/file');
  mocked.openUrl.mockResolvedValue(undefined);
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});
afterEach(cleanup);
it('uses the current account phone number in the title', () => {
  render(<App />);
  expect(document.title).toBe('19900001111 - CMCloud++');
});
const row = (name: string) => screen.getByText(name, { selector: 'strong' }).closest('.file-row')!;

it('keeps ordinary folder actions free of repository shortcuts', async () => {
  mocked.space = 'personal_new';
  mocked.probe.mockResolvedValue([{ folderId: 'folder', state: 'ordinary' }]);

  await act(async () => render(<App />));
  fireEvent.contextMenu(row('文件夹'));

  expect(screen.getByRole('menuitem', { name: '打开文件夹' })).toBeTruthy();
  expect(screen.queryByRole('menuitem', { name: '以普通文件夹打开' })).toBeNull();
  expect(screen.queryByRole('menuitem', { name: '建立 AxuanStor 对象库' })).toBeNull();
});

it('copies the folder ID from the context menu and omits the action for files', async () => {
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  try {
    render(<App />);
    fireEvent.contextMenu(row('文件夹'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制目录 ID' }));
    await screen.findByText('目录 ID 已复制');
    expect(writeText).toHaveBeenCalledExactlyOnceWith('folder');

    fireEvent.contextMenu(row('资料.zip'));
    expect(screen.queryByRole('menuitem', { name: '复制目录 ID' })).toBeNull();
  } finally {
    if (original) Object.defineProperty(navigator, 'clipboard', original);
    else Reflect.deleteProperty(navigator, 'clipboard');
  }
});

it('keeps the current cloud listing when repository opening is cancelled', async () => {
  mocked.space = 'personal_new';
  const repositoryId = 'a'.repeat(32);
  mocked.probe.mockResolvedValue([
    { folderId: 'folder', repositoryId, kind: 'plain', state: 'repository', saved: false },
  ]);
  mocked.list.mockResolvedValue({
    files: [
      { id: 'folder', name: '文件夹', path: '/folder', isDir: true, size: 0, modifiedAt: '' },
      { id: 'loaded', name: '刷新后才出现.txt', path: '/loaded', isDir: false, size: 1, modifiedAt: '' },
    ],
    nextCursor: null,
  });
  let finishOpen!: (value: RepositoryView) => void;
  mocked.openRepository.mockReturnValue(new Promise<RepositoryView>((resolve) => {
    finishOpen = resolve;
  }));
  mocked.closeRepository.mockResolvedValue(undefined);

  await act(async () => render(<App />));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '刷新目录' })));
  await act(async () => fireEvent.doubleClick(row('文件夹')));
  await waitFor(() => expect(mocked.openRepository).toHaveBeenCalledTimes(1));
  fireEvent.click(screen.getByRole('button', { name: '取消并返回' }));
  expect(row('刷新后才出现.txt')).toBeTruthy();

  await act(async () => finishOpen({
    handle: 'cancelled-handle', repositoryId, folderId: 'folder', revision: 'revision',
    directory: '', encrypted: false, historical: false, saved: false, items: [], conflicts: [],
  }));
  expect(mocked.closeRepository).toHaveBeenCalledWith('cancelled-handle');
  expect(row('刷新后才出现.txt')).toBeTruthy();
});

it.each(['plain', 'encrypted'] as const)(
  'loads a %s repository inside the management dialog over the existing list',
  async (kind) => {
    mocked.space = 'personal_new';
    const repositoryId = 'a'.repeat(32);
    mocked.probe.mockResolvedValue([
      { folderId: 'folder', repositoryId, kind, state: 'repository', saved: false },
    ]);
    let finishOpen!: (value: RepositoryView) => void;
    mocked.openRepository.mockReturnValue(new Promise<RepositoryView>((resolve) => {
      finishOpen = resolve;
    }));
    mocked.closeRepository.mockResolvedValue(undefined);
    mocked.labelCache.mockResolvedValue(undefined);

    await act(async () => render(<App />));
    fireEvent.contextMenu(row('文件夹'));
    fireEvent.click(screen.getByRole('menuitem', { name: '管理对象库' }));
    const dialog = screen.getByRole('dialog', { name: '管理对象库' });
    expect(row('资料.zip')).toBeTruthy();
    if (kind === 'encrypted') {
      expect(mocked.openRepository).not.toHaveBeenCalled();
      fireEvent.change(within(dialog).getByLabelText('对象库密码'), {
        target: { value: 'fixture' },
      });
      fireEvent.click(within(dialog).getByRole('button', { name: '解锁' }));
    } else {
      expect(within(dialog).getByRole('status').textContent).toContain('正在加载');
    }

    await act(async () => finishOpen({
      handle: 'management-handle',
      repositoryId,
      folderId: 'folder',
      revision: 'fixture-revision',
      encrypted: kind === 'encrypted',
      directory: '',
      items: [
        { name: '库内文件.txt', path: '库内文件.txt', kind: 'file', size: 4, conflict: false, protected: false },
      ],
      conflicts: [],
      historical: false,
      saved: false,
    }));
    expect(screen.getByRole('dialog', { name: '管理对象库' })).toBe(dialog);
    expect(within(dialog).getByText(repositoryId)).toBeTruthy();
    expect(row('资料.zip')).toBeTruthy();
    expect(screen.queryByText('库内文件.txt')).toBeNull();

    await act(async () => fireEvent.click(within(dialog).getByRole('button', { name: '完成' })));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(row('资料.zip')).toBeTruthy();
    expect(mocked.closeRepository).toHaveBeenCalledExactlyOnceWith('management-handle');
  },
);

it.each(['plain', 'encrypted'] as const)(
  'keeps a verified %s repository in the context menu after exiting',
  async (kind) => {
    mocked.space = 'personal_new';
    const repositoryId = 'a'.repeat(32);
    mocked.probe.mockResolvedValueOnce([
      { folderId: 'folder', repositoryId, kind, state: 'repository', saved: false },
    ]);
    let finishRefresh!: (rows: unknown[]) => void;
    mocked.probe.mockReturnValue(new Promise((resolve) => (finishRefresh = resolve)));
    mocked.openRepository.mockResolvedValue({
      handle: 'fixture-handle',
      repositoryId,
      folderId: 'folder',
      revision: 'fixture-revision',
      encrypted: kind === 'encrypted',
      directory: '',
      items: [],
      conflicts: [],
      historical: false,
      saved: false,
    });
    mocked.closeRepository.mockResolvedValue(undefined);
    mocked.labelCache.mockResolvedValue(undefined);

    await act(async () => render(<App />));
    fireEvent.contextMenu(row('文件夹'));
    await act(async () =>
      fireEvent.click(screen.getByRole('menuitem', { name: '打开 AxuanStor 对象库' })),
    );
    if (kind === 'encrypted') {
      fireEvent.change(screen.getByLabelText('对象库密码'), { target: { value: 'fixture' } });
      await act(async () => fireEvent.click(screen.getByRole('button', { name: '解锁' })));
    }
    await act(async () =>
      fireEvent.click(screen.getByRole('button', { name: '退出对象库' })),
    );
    fireEvent.contextMenu(row('文件夹'));
    expect(screen.getByRole('menuitem', { name: '打开 AxuanStor 对象库' })).toBeTruthy();
    expect(screen.queryByRole('menuitem', { name: '打开文件夹' })).toBeNull();

    if (kind === 'encrypted') {
      expect(mocked.probe).toHaveBeenCalledTimes(2);
      await act(async () =>
        finishRefresh([
          { folderId: 'folder', repositoryId: null, kind: null, state: 'unavailable', saved: false },
        ]),
      );
      expect(screen.getByRole('menuitem', { name: '打开 AxuanStor 对象库' })).toBeTruthy();
    }
  },
);

it('selects folders on a single desktop click and opens them on double-click', async () => {
  render(<App />);
  fireEvent.click(row('文件夹'));
  expect((screen.getByLabelText('选择 文件夹') as HTMLInputElement).checked).toBe(true);
  expect(mocked.list).not.toHaveBeenCalled();
  await act(async () => fireEvent.doubleClick(row('文件夹')));
  expect(screen.getByRole('button', { name: '返回上级目录' })).toBeTruthy();
  expect(document.querySelectorAll('.file-row')).toHaveLength(0);
});

it('clears selection on blank clicks, while controls and row metadata keep selecting files', () => {
  render(<App />);
  fireEvent.click(row('资料.zip').querySelector('.col-size')!);
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(true);
  fireEvent.click(document.querySelector('.virtual-body')!);
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(false);
  fireEvent.click(screen.getByLabelText('选择 资料.zip'));
  fireEvent.click(document.querySelector('.files-workspace')!);
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(false);
});

it('opens the archive directory in the app without opening an external link', async () => {
  render(<App />);
  await act(async () => fireEvent.doubleClick(row('资料.zip')));
  expect(screen.getByRole('region', { name: '资料.zip' })).toBeTruthy();
  expect(screen.getByRole('button', { name: '我的文件' })).toBeTruthy();
  expect(screen.queryByRole('dialog', { name: '资料.zip' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '说明.md 操作菜单' }));
  expect(screen.getByRole('menuitem', { name: '下载' })).toBeTruthy();
  expect(screen.getByRole('button', { name: '下载压缩包' })).toBeTruthy();
  expect(mocked.link).not.toHaveBeenCalled();
  expect(mocked.openUrl).not.toHaveBeenCalled();
});

it('supports Ctrl toggle and Shift range selection like the desktop file explorer', () => {
  render(<App />);
  fireEvent.click(row('文件夹'));
  fireEvent.click(row('资料.zip'), { ctrlKey: true });
  expect((screen.getByLabelText('选择 文件夹') as HTMLInputElement).checked).toBe(true);
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(true);
  fireEvent.click(row('资料.zip'), { ctrlKey: true });
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(false);
  fireEvent.click(row('文件夹'));
  fireEvent.click(row('资料.zip'), { shiftKey: true });
  for (const name of ['文件夹', '图片.png', '影片.mp4', '资料.zip']) {
    expect((screen.getByLabelText(`选择 ${name}`) as HTMLInputElement).checked).toBe(true);
  }
});

it('opens image previews in the app only on double-click and offers a download if decoding fails', async () => {
  render(<App />);
  fireEvent.click(row('图片.png'));
  expect(mocked.link).not.toHaveBeenCalled();
  await act(async () => fireEvent.doubleClick(row('图片.png')));
  expect(mocked.link).toHaveBeenCalledTimes(1);
  const preview = screen.getByRole('region', { name: '图片.png' });
  fireEvent.error(within(preview).getByRole('img', { name: '图片.png' }));
  expect(within(preview).getByRole('alert').textContent).toContain('请下载');
  expect(mocked.openUrl).not.toHaveBeenCalled();
});

it('shows a download action when video preview is unavailable', async () => {
  mocked.preview.mockRejectedValue(new Error('unsupported'));
  mocked.link.mockRejectedValueOnce(new Error('download source unavailable'));
  render(<App />);
  await act(async () => fireEvent.doubleClick(row('影片.mp4')));
  expect(screen.getByRole('alert').textContent).toContain('请下载');
  expect(screen.getByRole('button', { name: '下载文件' })).toBeTruthy();
});

it('provides six transfer categories and shows error filtering only for ongoing categories', () => {
  render(<App />);
  fireEvent.click(screen.getByRole('button', { name: '传输列表' }));
  for (const name of ['上传中', '已上传', '下载中', '已下载', '全部进行中', '全部已完成']) {
    expect(screen.getByRole('button', { name })).toBeTruthy();
  }
  expect(screen.getByRole('checkbox', { name: '仅显示出错项' })).toBeTruthy();
  fireEvent.click(screen.getByRole('checkbox', { name: '仅显示出错项' }));
  expect(screen.getByRole('heading', { name: '没有出错的任务' })).toBeTruthy();
  expect(screen.queryByText('在文件列表中上传或下载，任务会显示在这里。')).toBeNull();
  fireEvent.click(screen.getByRole('checkbox', { name: '仅显示出错项' }));
  expect(screen.getByRole('progressbar', { name: '总进度' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '已上传' }));
  expect(screen.queryByRole('checkbox', { name: '仅显示出错项' })).toBeNull();
  expect(screen.queryByRole('progressbar')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '下载中' }));
  expect(screen.getByRole('progressbar', { name: '总下载进度' })).toBeTruthy();
});

it('keeps branding noninteractive and stores the vendor-folder preference', () => {
  render(<App />);
  expect(document.querySelector('.brand')?.tagName).toBe('DIV');
  expect(screen.getByRole('button', { name: '收起' })).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '设置' }));
  const toggle = screen.getByRole('switch', { name: '隐藏厂商文件夹' });
  expect(toggle.getAttribute('aria-checked')).toBe('true');
  fireEvent.click(toggle);
  expect(localStorage.getItem('cmcloud.hideVendorFolders')).toBe('false');
  fireEvent.click(document.querySelector('.brand')!);
  expect(screen.getByRole('button', { name: '账号管理' })).toBeTruthy();
  expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
});

it('offers directory actions on blank space without replacing file menus', () => {
  render(<App />);
  fireEvent.click(row('资料.zip'));
  fireEvent.contextMenu(document.querySelector('.virtual-body')!, { clientX: 100, clientY: 150 });
  const menu = screen.getByRole('menu');
  expect(
    within(menu)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['刷新', '复制当前路径', '上传文件夹', '上传文件', '新建文件夹', '新建加密文件夹']);
  expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(false);
  fireEvent.click(within(menu).getByRole('menuitem', { name: '新建文件夹' }));
  expect(screen.getByRole('dialog').textContent).toContain('新建文件夹');
});

it('copies current, folder, and file paths in cloud folders and inside object repositories', async () => {
  const original = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  mocked.space = 'personal_new';
  const repositoryId = '86834bac65b014b1c05cf953ffc01d46';
  mocked.probe.mockImplementation(async (ids: string[]) =>
    ids.map((id) =>
      id === 'x3-folder-id'
        ? { folderId: id, repositoryId, kind: 'plain', state: 'repository', saved: false }
        : { folderId: id, state: 'ordinary' },
    ),
  );
  mocked.list.mockImplementation(async (dir: { id: string }) => {
    if (dir.id === '/') {
      return {
        files: [
          { id: 'objects-id', name: 'objects', path: '/', isDir: true, size: 0, modifiedAt: '' },
          { id: 'zip', name: '资料.zip', path: '/', isDir: false, size: 10, modifiedAt: '' },
        ],
        nextCursor: null,
      };
    }
    return {
      files: [
        { id: 'x3-folder-id', name: 'x3-client', path: 'objects-id', isDir: true, size: 0, modifiedAt: '' },
      ],
      nextCursor: null,
    };
  });
  mocked.openRepository.mockResolvedValue({
    handle: 'repo-handle',
    repositoryId,
    folderId: 'x3-folder-id',
    revision: 'rev-1',
    encrypted: false,
    directory: '',
    items: [
      { name: '1.7.1300', path: '1.7.1300', kind: 'directory', size: 0, conflict: false, protected: false },
    ],
    conflicts: [],
    historical: false,
    saved: false,
  });
  mocked.listRepository.mockImplementation(async (_handle: string, directory: string) => ({
    handle: 'repo-handle',
    repositoryId,
    folderId: 'x3-folder-id',
    revision: 'rev-1',
    encrypted: false,
    directory,
    items:
      directory === '1.7.1300'
        ? [
            {
              name: 'Total',
              path: '1.7.1300/Total',
              kind: 'directory',
              size: 0,
              conflict: false,
              protected: false,
            },
          ]
        : [
            {
              name: 'Audio',
              path: '1.7.1300/Total/Audio',
              kind: 'directory',
              size: 0,
              conflict: false,
              protected: false,
            },
            {
              name: 'bgm.ab',
              path: '1.7.1300/Total/bgm.ab',
              kind: 'file',
              size: 128,
              conflict: false,
              protected: false,
            },
          ],
    conflicts: [],
    historical: false,
    saved: false,
  }));
  mocked.closeRepository.mockResolvedValue(undefined);
  mocked.labelCache.mockResolvedValue(undefined);

  try {
    await act(async () => render(<App />));
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '刷新目录' })));

    // Cloud root: blank space, folder, file
    fireEvent.contextMenu(document.querySelector('.virtual-body')!);
    fireEvent.click(screen.getByRole('menuitem', { name: '复制当前路径' }));
    await screen.findByText('当前路径已复制');
    expect(writeText).toHaveBeenLastCalledWith('/');

    fireEvent.contextMenu(row('objects'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件夹路径' }));
    await screen.findByText('文件夹路径已复制');
    expect(writeText).toHaveBeenLastCalledWith('/objects');

    fireEvent.contextMenu(row('资料.zip'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件路径' }));
    await screen.findByText('文件路径已复制');
    expect(writeText).toHaveBeenLastCalledWith('/资料.zip');

    // Enter /objects and check x3-client folder path before entering repository
    await act(async () => fireEvent.doubleClick(row('objects')));
    await act(async () => fireEvent.click(screen.getByRole('button', { name: '刷新目录' })));
    await waitFor(() => expect(row('x3-client')).toBeTruthy());
    fireEvent.contextMenu(document.querySelector('.virtual-body')!);
    fireEvent.click(screen.getByRole('menuitem', { name: '复制当前路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects');

    fireEvent.contextMenu(row('x3-client'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件夹路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client');

    // Enter object repository /objects/x3-client -> 1.7.1300 -> Total
    await act(async () => fireEvent.doubleClick(row('x3-client')));
    await waitFor(() => expect(row('1.7.1300')).toBeTruthy());
    fireEvent.contextMenu(document.querySelector('.virtual-body')!);
    fireEvent.click(screen.getByRole('menuitem', { name: '复制当前路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client:');

    await act(async () => fireEvent.doubleClick(row('1.7.1300')));
    await waitFor(() => expect(row('Total')).toBeTruthy());
    fireEvent.contextMenu(row('Total'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件夹路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client:1.7.1300/Total');

    await act(async () => fireEvent.doubleClick(row('Total')));
    await waitFor(() => expect(row('bgm.ab')).toBeTruthy());
    fireEvent.contextMenu(document.querySelector('.virtual-body')!);
    fireEvent.click(screen.getByRole('menuitem', { name: '复制当前路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client:1.7.1300/Total');

    fireEvent.contextMenu(row('Audio'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件夹路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client:1.7.1300/Total/Audio');

    fireEvent.contextMenu(row('bgm.ab'));
    fireEvent.click(screen.getByRole('menuitem', { name: '复制文件路径' }));
    expect(writeText).toHaveBeenLastCalledWith('/objects/x3-client:1.7.1300/Total/bgm.ab');
  } finally {
    if (original) Object.defineProperty(navigator, 'clipboard', original);
    else Reflect.deleteProperty(navigator, 'clipboard');
  }
});

it('renders four mobile destinations and puts sessions inside settings', () => {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query === '(max-width: 700px)',
    addEventListener() {},
    removeEventListener() {},
  }));
  try {
    render(<App />);
    const nav = screen.getByRole('navigation', { name: '主导航' });
    expect(
      within(nav)
        .getAllByRole('button')
        .map((item) => item.textContent),
    ).toEqual(['我的文件', '传输列表', '群组与分享', '设置']);
    fireEvent.click(within(nav).getByRole('button', { name: '设置' }));
    expect(screen.queryByRole('region', { name: '账号管理' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '账号管理' }));
    expect(screen.getByRole('region', { name: '账号管理' })).toBeTruthy();
    expect(screen.getByText('19900001111', { selector: 'strong' })).toBeTruthy();
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
    expect(document.querySelector('.app-shell.mobile-layout')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '返回设置' }));
    expect(screen.queryByRole('region', { name: '账号管理' })).toBeNull();
    expect(screen.getByRole('switch', { name: '隐藏厂商文件夹' })).toBeTruthy();
  } finally {
    vi.unstubAllGlobals();
  }
});

it('opens archives with one touch and toggles folders while choosing files', async () => {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query === '(max-width: 700px)',
    addEventListener() {},
    removeEventListener() {},
  }));
  try {
    render(<App />);
    await act(async () => fireEvent.click(row('资料.zip')));
    expect(screen.getByRole('region', { name: '资料.zip' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '关闭文件预览' }));
    expect(screen.queryByRole('button', { name: '选择' })).toBeNull();
    fireEvent.contextMenu(row('文件夹'));
    expect(screen.queryByRole('menu')).toBeNull();
    fireEvent.click(row('文件夹'));
    fireEvent.click(row('资料.zip'));
    expect((screen.getByLabelText('选择 文件夹') as HTMLInputElement).checked).toBe(true);
    expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(true);
    expect(mocked.list).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '取消选择' }));
    expect(document.querySelector('.selecting-files')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '添加' }));
    expect(
      within(screen.getByRole('menu')).getByRole('menuitem', { name: '上传文件夹' }),
    ).toBeTruthy();
    expect(screen.getByRole('dialog', { name: '目录操作' })).toHaveProperty('open', true);
  } finally {
    vi.unstubAllGlobals();
  }
});

it('exits mobile selection mode automatically when the last selected item is deselected', () => {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query === '(max-width: 700px)',
    addEventListener() {},
    removeEventListener() {},
  }));
  try {
    render(<App />);
    // Simulate touch long-press (contextMenu + finger-up synthetic click)
    fireEvent.contextMenu(row('文件夹'));
    fireEvent.click(row('文件夹'));
    expect(document.querySelector('.selecting-files')).toBeTruthy();
    expect((screen.getByLabelText('选择 文件夹') as HTMLInputElement).checked).toBe(true);

    // Select a second item, then deselect it -> still in selection mode (1 item remains)
    fireEvent.click(row('资料.zip'));
    expect((screen.getByLabelText('选择 资料.zip') as HTMLInputElement).checked).toBe(true);
    fireEvent.click(row('资料.zip'));
    expect(document.querySelector('.selecting-files')).toBeTruthy();

    // Deselect the last remaining item -> automatically exits selection mode
    fireEvent.click(row('文件夹'));
    expect((screen.getByLabelText('选择 文件夹') as HTMLInputElement).checked).toBe(false);
    expect(document.querySelector('.selecting-files')).toBeNull();
    expect(screen.getByRole('button', { name: '添加' })).toBeTruthy();
  } finally {
    vi.unstubAllGlobals();
  }
});

it('enters path editing on address bar blank click and navigates cloud and object repository paths', async () => {
  mocked.space = 'personal_new';
  const repositoryId = '86834bac65b014b1c05cf953ffc01d46';
  const objectsDir = {
    id: 'objects-id',
    name: 'objects',
    path: 'root',
    isDir: true,
    size: 0,
    modifiedAt: '2026-11-24T12:00:00Z',
  };
  const x3RepoDir = {
    id: 'x3-folder-id',
    name: 'x3-client',
    path: 'objects-id',
    isDir: true,
    size: 0,
    modifiedAt: '2026-11-24T12:00:00Z',
  };
  mocked.list.mockImplementation(async (dir: { id: string }) => {
    if (dir.id === 'objects-id') return { files: [x3RepoDir], nextCursor: null };
    return {
      files: [
        objectsDir,
        {
          id: 'zip-id',
          name: '资料.zip',
          path: 'root',
          isDir: false,
          size: 1024,
          modifiedAt: '2026-11-24T12:00:00Z',
        },
      ],
      nextCursor: null,
    };
  });
  mocked.probe.mockImplementation(async (ids: string[]) =>
    ids.map((id) => ({
      folderId: id,
      repositoryId: id === 'x3-folder-id' ? repositoryId : null,
      kind: id === 'x3-folder-id' ? 'plain' : null,
      state: id === 'x3-folder-id' ? 'repository' : 'ordinary',
      saved: false,
    })),
  );
  mocked.openRepository.mockResolvedValue({
    handle: 'repo-handle',
    repositoryId,
    folderId: 'x3-folder-id',
    revision: 'rev-1',
    encrypted: false,
    directory: '',
    items: [{ name: '1.7.1300', path: '1.7.1300', kind: 'directory', size: 0, conflict: false, protected: false }],
    conflicts: [],
    historical: false,
    saved: false,
  });
  mocked.listRepository.mockImplementation(async (_handle: string, directory: string) => ({
    handle: 'repo-handle',
    repositoryId,
    folderId: 'x3-folder-id',
    revision: 'rev-1',
    encrypted: false,
    directory,
    items:
      directory === '1.7.1300/Total'
        ? [
            {
              name: 'bgm.ab',
              path: '1.7.1300/Total/bgm.ab',
              kind: 'file',
              size: 128,
              conflict: false,
              protected: false,
            },
          ]
        : directory === '1.7.1300'
          ? [
              {
                name: 'Total',
                path: '1.7.1300/Total',
                kind: 'directory',
                size: 0,
                conflict: false,
                protected: false,
              },
            ]
          : [],
    conflicts: [],
    historical: false,
    saved: false,
  }));
  mocked.closeRepository.mockResolvedValue(undefined);
  mocked.labelCache.mockResolvedValue(undefined);

  await act(async () => render(<App />));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '刷新目录' })));

  // Click blank space in address bar -> opens path input with '/'
  fireEvent.click(document.querySelector('.address-bar-blank')!);
  const input = screen.getByRole('textbox', { name: '路径地址栏' }) as HTMLInputElement;
  expect(input.value).toBe('/');
  fireEvent.keyDown(input, { key: 'Escape' });
  expect(screen.queryByRole('textbox', { name: '路径地址栏' })).toBeNull();

  // Jump directly from '/' into '/objects/x3-client:1.7.1300/Total'
  fireEvent.click(document.querySelector('.address-bar-blank')!);
  const repoInput = screen.getByRole('textbox', { name: '路径地址栏' });
  fireEvent.change(repoInput, { target: { value: '/objects/x3-client:1.7.1300/Total' } });
  await act(async () => fireEvent.submit(repoInput.closest('form')!));
  await waitFor(() => expect(row('bgm.ab')).toBeTruthy());

  // Click blank space inside object repository -> shows '/objects/x3-client:1.7.1300/Total'
  fireEvent.click(document.querySelector('.address-bar-blank')!);
  const insideInput = screen.getByRole('textbox', { name: '路径地址栏' }) as HTMLInputElement;
  expect(insideInput.value).toBe('/objects/x3-client:1.7.1300/Total');

  // Jump back out of object repository to '/objects'
  fireEvent.change(insideInput, { target: { value: '/objects' } });
  await act(async () => fireEvent.submit(insideInput.closest('form')!));
  await waitFor(() => expect(row('x3-client')).toBeTruthy());
});

it('shows OLED only in dark mode and preserves its preference across light mode and restart', () => {
  const app = render(<App />);
  fireEvent.click(screen.getByRole('button', { name: '设置' }));
  expect(screen.queryByRole('switch', { name: 'OLED 纯黑' })).toBeNull();
  const mode = screen.getByRole('combobox', { name: '外观模式' });
  expect([...mode.querySelectorAll('option')].map((item) => item.textContent)).toEqual([
    '跟随系统',
    '浅色模式',
    '深色模式',
  ]);
  fireEvent.change(mode, { target: { value: 'dark' } });
  fireEvent.click(screen.getByRole('switch', { name: 'OLED 纯黑' }));
  expect(document.documentElement.dataset.colorScheme).toBe('oled');
  expect(localStorage.getItem('cmcloud.oledPureBlack')).toBe('true');
  fireEvent.change(mode, { target: { value: 'light' } });
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(document.documentElement.dataset.colorScheme).toBe('classic');
  expect(screen.queryByRole('switch', { name: 'OLED 纯黑' })).toBeNull();
  app.unmount();
  render(<App />);
  expect(document.documentElement.dataset.theme).toBe('light');
  fireEvent.click(screen.getByRole('button', { name: '设置' }));
  fireEvent.change(screen.getByRole('combobox', { name: '外观模式' }), {
    target: { value: 'dark' },
  });
  expect(document.documentElement.dataset.colorScheme).toBe('oled');
  expect(screen.getByRole('switch', { name: 'OLED 纯黑' }).getAttribute('aria-checked')).toBe(
    'true',
  );
});

it('follows system changes while preserving the system and OLED preferences', () => {
  let dark = false;
  let changed = () => {};
  vi.stubGlobal('matchMedia', (query: string) => ({
    get matches() {
      return query === '(prefers-color-scheme: dark)' && dark;
    },
    addEventListener(_name: string, listener: () => void) {
      if (query === '(prefers-color-scheme: dark)') changed = listener;
    },
    removeEventListener() {},
  }));
  try {
    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: '设置' }));
    expect(screen.queryByRole('switch', { name: '上报真实文件大小' })).toBeNull();
    fireEvent.change(screen.getByRole('combobox', { name: '外观模式' }), {
      target: { value: 'system' },
    });
    expect(document.documentElement.dataset.theme).toBe('light');
    act(() => {
      dark = true;
      changed();
    });
    expect(document.documentElement.dataset.theme).toBe('dark');
    fireEvent.click(screen.getByRole('switch', { name: 'OLED 纯黑' }));
    expect(document.documentElement.dataset.colorScheme).toBe('oled');
    act(() => {
      dark = false;
      changed();
    });
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(document.documentElement.dataset.colorScheme).toBe('classic');
    expect(screen.queryByRole('switch', { name: 'OLED 纯黑' })).toBeNull();
    expect(localStorage.getItem('cmcloud.theme')).toBe('system');
  } finally {
    vi.unstubAllGlobals();
  }
});

it('sorts the working list by size and modified time in both directions', () => {
  render(<App />);
  const names = () =>
    [...document.querySelectorAll('.file-row strong')].map((node) => node.textContent);
  fireEvent.click(screen.getByRole('button', { name: '大小' }));
  expect(names()).toEqual(['文件夹', '资料.zip', '图片.png', '影片.mp4']);
  fireEvent.click(screen.getByRole('button', { name: '大小' }));
  expect(names()).toEqual(['文件夹', '影片.mp4', '图片.png', '资料.zip']);
  const modified = screen.getByRole('button', { name: '修改时间' });
  fireEvent.click(modified);
  expect(names()).toEqual(['文件夹', '资料.zip', '影片.mp4', '图片.png']);
  fireEvent.click(modified);
  expect(names()).toEqual(['文件夹', '图片.png', '影片.mp4', '资料.zip']);
});

it('keeps loaded-file search local until recursive search is requested and restores it on clear', async () => {
  mocked.list.mockResolvedValue({
    files: [
      { id: 'deep', name: '深层结果.zip', path: '/deep', isDir: false, size: 1, modifiedAt: '' },
    ],
  });
  render(<App />);
  expect(screen.getByPlaceholderText('搜索')).toBeTruthy();
  fireEvent.change(screen.getByPlaceholderText('搜索'), { target: { value: '深层' } });
  expect(mocked.list).not.toHaveBeenCalled();
  await act(async () =>
    fireEvent.click(screen.getByRole('button', { name: '递归搜索当前文件夹' })),
  );
  expect(screen.getByText('深层结果.zip', { selector: 'strong' })).toBeTruthy();
  fireEvent.change(screen.getByPlaceholderText('搜索'), { target: { value: '' } });
  expect(screen.queryByText('深层结果.zip', { selector: 'strong' })).toBeNull();
  expect(row('文件夹')).toBeTruthy();
});

it('keeps the loaded count inside the scroll area after the files and omits folder size in grid view', () => {
  render(<App />);
  const scroll = document.querySelector('.file-scroll')!;
  expect(scroll.lastElementChild?.classList.contains('file-footer')).toBe(true);
  expect(screen.queryByRole('combobox', { name: '排序方式' })).toBeNull();
  const mode = screen.getByRole('combobox', { name: '显示方式' });
  expect(within(mode).getAllByRole('option').map((item) => item.textContent))
    .toEqual(['列表', '大图标', '中图标', '小图标']);
  fireEvent.change(mode, { target: { value: 'grid-medium' } });
  expect(row('文件夹').closest('.grid-medium')).toBeTruthy();
  fireEvent.change(mode, { target: { value: 'grid-small' } });
  expect(row('文件夹').closest('.grid-small')).toBeTruthy();
  expect(row('文件夹').querySelector('.col-size')).toBeNull();
  expect(row('文件夹').querySelector('small')).toBeNull();
  expect(row('资料.zip').querySelector('.col-size')?.textContent).toContain('10');
});

it('handles Android back by returning to the parent folder before backgrounding the app', () => {
  render(<App />);
  fireEvent.doubleClick(row('文件夹'));
  expect(screen.getByRole('button', { name: '返回上级目录' })).toBeTruthy();
  const back = new Event('cmcloud-back', { cancelable: true });
  act(() => {
    window.dispatchEvent(back);
  });
  expect(back.defaultPrevented).toBe(true);
  expect(screen.queryByRole('button', { name: '返回上级目录' })).toBeNull();
  const rootBack = new Event('cmcloud-back', { cancelable: true });
  act(() => {
    window.dispatchEvent(rootBack);
  });
  expect(rootBack.defaultPrevented).toBe(false);
});

it('separates closing mobile search from clearing its text and groups view controls in a menu', () => {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query === '(max-width: 700px)',
    addEventListener() {},
    removeEventListener() {},
  }));
  try {
    render(<App />);
    expect(screen.getByRole('button', { name: '添加' }).textContent).toBe('');
    expect(screen.queryByRole('combobox', { name: '文件类型' })).toBeNull();
    const input = screen.getByPlaceholderText('搜索');
    fireEvent.focus(input);
    expect(screen.queryByRole('button', { name: '添加' })).toBeNull();
    fireEvent.change(input, { target: { value: '资料' } });
    fireEvent.click(screen.getByRole('button', { name: '清除搜索' }));
    expect((input as HTMLInputElement).value).toBe('');
    expect(screen.getByRole('button', { name: '关闭搜索' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '关闭搜索' }));
    expect(screen.getByRole('button', { name: '添加' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: '显示选项' }));
    const menu = screen.getByRole('dialog', { name: '文件显示选项' });
    fireEvent.pointerDown(menu);
    expect(menu).toHaveProperty('open', true);
    fireEvent.click(within(menu).getByRole('button', { name: '大图标' }));
    expect(document.querySelector('.icon-view')).toBeTruthy();
    const sort = within(within(menu).getByRole('group', { name: '排序方式' }));
    fireEvent.click(sort.getByRole('button', { name: '大小' }));
    expect(sort.getByRole('button', { name: '大小' }).getAttribute('aria-pressed')).toBe('true');
    const descending = within(menu).getByRole('button', { name: '降序' });
    fireEvent.click(descending);
    expect(descending.getAttribute('aria-pressed')).toBe('true');
    fireEvent.change(within(menu).getByRole('combobox', { name: '文件类型' }), {
      target: { value: 'folders' },
    });
    expect(document.querySelectorAll('.file-row')).toHaveLength(1);
    fireEvent.pointerDown(menu);
    fireEvent.click(menu);
    expect(screen.queryByRole('dialog', { name: '文件显示选项' })).toBeNull();
  } finally {
    vi.unstubAllGlobals();
  }
});

it('does not cancel from the toolbar until confirmation and sends the selected direction', async () => {
  mocked.previewJobs = [
    {
      id: 'up',
      name: 'upload',
      direction: 'upload',
      status: 'paused',
      size: 10,
      completed: 0,
      speed: 0,
      localPath: '',
      createdAt: '',
    },
    {
      id: 'down',
      name: 'download',
      direction: 'download',
      status: 'running',
      size: 10,
      completed: 0,
      speed: 0,
      localPath: '',
      createdAt: '',
    },
  ];
  render(<App />);
  fireEvent.click(screen.getByRole('button', { name: /传输列表/ }));
  fireEvent.click(screen.getByRole('button', { name: '取消全部' }));
  expect(mocked.controlBatch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '保留任务' }));
  expect(mocked.controlBatch).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '上传中' }));
  fireEvent.click(screen.getByRole('button', { name: '取消上传' }));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '确认取消上传' })));
  expect(mocked.controlBatch).toHaveBeenCalledExactlyOnceWith('cancel', 'upload');
  expect(screen.queryByRole('dialog', { name: '取消上传？' })).toBeNull();
});

it('stays in the outer directory after creating a folder, object repository, or encrypted folder', async () => {
  mocked.space = 'personal_new';
  mocked.probe.mockResolvedValue([{ folderId: 'folder', state: 'ordinary' }]);
  const outerFiles = [
    { id: 'folder', name: '文件夹', path: '/folder', isDir: true, size: 0, modifiedAt: '' },
  ];
  mocked.list.mockImplementation(async () => ({ files: [...outerFiles], nextCursor: null }));
  mocked.mkdir.mockImplementation(async (_parent, name: string) => {
    outerFiles.push({ id: `dir:${name}`, name, path: `/${name}`, isDir: true, size: 0, modifiedAt: '' });
  });
  mocked.createNamedRepository.mockImplementation(async (_parent, name: string) => ({
    handle: 'repo-create-handle',
    folderId: 'repo-created-id',
    repositoryId: 'repo-guid-1',
    name,
    encrypted: false,
    directory: '',
    revision: 'rev-1',
    unlockRevision: 'unlock-1',
    policy: 'normal',
    saved: false,
    unlockMethods: [],
    items: [],
  }));
  mocked.createCrypt.mockImplementation(async (_parent, name: string) => ({
    handle: 'crypt-create-handle',
    folderId: 'crypt-created-id',
    name,
    directory: '',
    saved: true,
    options: {
      filenameEncryption: 'standard',
      directoryNameEncryption: true,
      encryptedSuffix: '.bin',
      filenameEncoding: 'base64',
    },
    items: [],
  }));

  await act(async () => render(<App />));

  // 1. Regular folder creation stays in outer directory
  fireEvent.click(screen.getByRole('button', { name: '新建文件夹' }));
  const mkdirDialog = screen.getByRole('dialog');
  fireEvent.change(within(mkdirDialog).getByLabelText('名称'), { target: { value: '普通新目录' } });
  await act(async () => {
    fireEvent.click(within(mkdirDialog).getByRole('button', { name: '确认' }));
  });
  expect(screen.queryByRole('button', { name: '退出对象库' })).toBeNull();
  expect(screen.getByText('普通新目录', { selector: 'strong' })).toBeTruthy();

  // 2. Object repository creation stays in outer directory and closes temporary session handle
  fireEvent.click(screen.getByRole('button', { name: '新建对象库' }));
  const repoDialog = screen.getByRole('dialog');
  fireEvent.change(within(repoDialog).getByLabelText('对象库名称'), {
    target: { value: '新建普通库' },
  });
  fireEvent.change(within(repoDialog).getByLabelText('对象库类型'), {
    target: { value: 'plain' },
  });
  await act(async () => {
    fireEvent.click(within(repoDialog).getByRole('button', { name: '创建对象库' }));
  });
  expect(mocked.closeRepository).toHaveBeenCalledWith('repo-create-handle');
  expect(screen.queryByRole('button', { name: '退出对象库' })).toBeNull();
  expect(within(document.querySelector<HTMLElement>('.address-bar')!).queryByText('新建普通库')).toBeNull();
  expect(within(row('新建普通库') as HTMLElement).getByRole('img', { name: 'AxuanStor 对象库' })).toBeTruthy();
  expect(screen.getByText('普通新目录', { selector: 'strong' })).toBeTruthy();

  // 3. Encrypted folder creation stays in outer directory and closes temporary session handle
  fireEvent.click(screen.getByRole('button', { name: '新建加密文件夹' }));
  const cryptDialog = screen.getByRole('dialog');
  fireEvent.change(within(cryptDialog).getByLabelText('文件夹名称'), {
    target: { value: '新建加密目录' },
  });
  fireEvent.change(within(cryptDialog).getByLabelText('加密密码 (password)'), {
    target: { value: 'correct-horse-battery' },
  });
  fireEvent.change(within(cryptDialog).getByLabelText('确认密码'), {
    target: { value: 'correct-horse-battery' },
  });
  await act(async () => {
    fireEvent.click(within(cryptDialog).getByRole('button', { name: /创建加密文件夹/ }));
  });
  expect(mocked.closeCrypt).toHaveBeenCalledWith('crypt-create-handle');
  expect(screen.queryByRole('button', { name: '退出加密文件夹' })).toBeNull();
  expect(within(document.querySelector<HTMLElement>('.address-bar')!).queryByText('新建加密目录')).toBeNull();
  expect(within(row('新建加密目录') as HTMLElement).getByRole('img', { name: /加密文件夹/ })).toBeTruthy();
  expect(screen.getByText('新建普通库', { selector: 'strong' })).toBeTruthy();
});

