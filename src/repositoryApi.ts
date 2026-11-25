import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { native, normalizeJob } from './api';
import type { RawJob } from './api';
import type { CloudFile } from './types';

export interface RepositoryProbe {
  folderId: string;
  repositoryId: string | null;
  kind: 'plain' | 'encrypted' | null;
  state: 'ordinary' | 'repository' | 'unavailable';
  saved: boolean;
}
export interface RepositoryItem {
  name: string;
  path: string;
  kind: 'file' | 'directory';
  size: number;
  sha256?: string;
  modifiedAt?: number;
  conflict: boolean;
  protected: boolean;
}
export interface RepositoryView {
  handle: string;
  repositoryId: string;
  folderId: string;
  revision: string;
  encrypted: boolean;
  directory: string;
  items: RepositoryItem[];
  conflicts: string[];
  historical: boolean;
  saved: boolean;
  credentialRevision?: string;
  unlockMethods?: string[];
  canChangeUnlock?: boolean;
}
export interface RepositoryUnlock {
  password?: string;
  key?: string;
  keyFile?: string | null;
}
export interface RepositoryUploadPlan {
  directories?: number;
  skipped?: number;
  id: string;
  items: { path: string; size: number; action: string; sha256: string }[];
  logicalBytes: number;
  sourceBytes: number;
  uploadBytes: number;
  uniqueUploads: number;
}
export interface RepositoryCache {
  id: string;
  name: string;
  repositoryId: string;
  bytes: number;
  busy: boolean;
}
export interface RepositoryPreviewStart {
  url: string | null;
  job: RawJob | null;
  streamId?: string | null;
}
export async function watchRepositoryDirectory(callback: (repositoryId: string) => void) {
  if (!native) return () => {};
  return listen<{ repositoryId: string }>('repository-directory-changed', (event) =>
    callback(event.payload.repositoryId),
  );
}
export const repositoryPreviewUrl = (path: string) => convertFileSrc(path);
export const repositoryDemo =
  !native && new URLSearchParams(location.search).get('preview') === 'scale';
const demoProbes = new Map<string, RepositoryProbe>([
  [
    'fixture-0',
    {
      folderId: 'fixture-0',
      repositoryId: 'a'.repeat(32),
      kind: 'plain',
      state: 'repository',
      saved: false,
    },
  ],
  [
    'fixture-4',
    {
      folderId: 'fixture-4',
      repositoryId: 'b'.repeat(32),
      kind: 'encrypted',
      state: 'repository',
      saved: false,
    },
  ],
]);
const demoSessions = new Map<string, RepositoryView>();
const demoCaches: RepositoryCache[] = [
  {
    id: 'c'.repeat(64),
    repositoryId: 'a'.repeat(32),
    name: '归档',
    bytes: 32 * 1024 * 1024,
    busy: false,
  },
  {
    id: 'd'.repeat(64),
    repositoryId: 'b'.repeat(32),
    name: '私密资料',
    bytes: 8 * 1024 * 1024,
    busy: false,
  },
];
const demoItems: RepositoryItem[] = [
  { name: '文档', path: '文档', kind: 'directory', size: 0, conflict: false, protected: false },
  { name: '照片', path: '照片', kind: 'directory', size: 0, conflict: false, protected: false },
  {
    name: '使用说明.txt',
    path: '使用说明.txt',
    kind: 'file',
    size: 2048,
    sha256: 'c'.repeat(64),
    modifiedAt: Date.parse('2026-11-11T00:00:00+08:00'),
    conflict: false,
    protected: false,
  },
];
function demoView(folder: string): RepositoryView {
  const p = demoProbes.get(folder)!;
  return {
    handle: crypto.randomUUID(),
    repositoryId: p.repositoryId!,
    folderId: folder,
    revision: 'demo-revision',
    encrypted: p.kind === 'encrypted',
    directory: '',
    items: demoDirectory(p.repositoryId!, ''),
    conflicts: [],
    historical: false,
    saved: p.saved,
    credentialRevision: 'demo-credentials',
    unlockMethods: p.kind === 'encrypted' ? ['password'] : [],
    canChangeUnlock: p.kind === 'encrypted',
  };
}
const demoTrees = new Map<string, Map<string, RepositoryItem>>();
function demoTree(repository: string) {
  let tree = demoTrees.get(repository);
  if (!tree) {
    tree = new Map(
      [
        ...demoItems,
        {
          name: '合同.pdf',
          path: '文档/合同.pdf',
          kind: 'file' as const,
          size: 128 * 1024,
          sha256: 'e'.repeat(64),
          conflict: false,
          protected: false,
        },
      ].map((item) => [item.path, { ...item }]),
    );
    demoTrees.set(repository, tree);
  }
  return tree;
}
function demoDirectory(repository: string, directory: string) {
  return [...demoTree(repository).values()]
    .filter((item) => item.path.split('/').slice(0, -1).join('/') === directory)
    .map((item) => ({ ...item }));
}
function demoJob(
  name: string,
  direction: 'upload' | 'download',
  bytes: number,
  status = 'queued',
): RawJob {
  return {
    id: crypto.randomUUID(),
    name,
    direction,
    status,
    totalBytes: bytes,
    completedBytes: 0,
    speed: 0,
    localPath: `R:\\Temp\\CMCloud-Preview\\${name}`,
    createdAt: Date.now() / 1000,
  };
}
async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (native) return invoke<T>(command, args);
  if (!repositoryDemo) throw new Error('请在桌面或移动应用中管理对象库。');
  if (command === 'preview_repository_file')
    return { url: 'https://example.invalid/preview-file', job: null } as T;
  if (command === 'release_repository_preview') return undefined as T;
  if (command === 'list_repository_caches')
    return demoCaches.map((row) => ({
      ...row,
      busy: [...demoSessions.values()].some((view) => view.repositoryId === row.repositoryId),
    })) as T;
  if (command === 'delete_repository_cache') {
    const index = demoCaches.findIndex((row) => row.id === args.id);
    if (index >= 0) {
      if (
        [...demoSessions.values()].some(
          (view) => view.repositoryId === demoCaches[index].repositoryId,
        )
      )
        throw new Error('请先退出该对象库');
      demoCaches.splice(index, 1);
    }
    return undefined as T;
  }
  if (command === 'label_repository_cache') return undefined as T;
  if (command === 'forget_all_repository_credentials') {
    demoSessions.clear();
    for (const probe of demoProbes.values()) probe.saved = false;
    return 2 as T;
  }
  if (command === 'probe_repositories')
    return (args.folderIds as string[]).map(
      (folderId) =>
        demoProbes.get(folderId) ?? {
          folderId,
          repositoryId: null,
          kind: null,
          state: 'ordinary',
          saved: false,
        },
    ) as T;
  if (command === 'open_repository') {
    const p = demoProbes.get(String(args.folderId));
    if (!p) throw new Error('示例对象库不存在');
    if (
      p.kind === 'encrypted' &&
      !args.useSaved &&
      (args.unlock as RepositoryUnlock)?.password !== 'demo' &&
      !(args.unlock as RepositoryUnlock)?.keyFile
    )
      throw new Error('演示密码为 demo');
    const v = demoView(p.folderId);
    v.saved = p.saved = !!args.remember || p.saved;
    demoSessions.set(v.handle, v);
    return v as T;
  }
  if (command === 'create_repository' || command === 'create_named_repository') {
    const p: RepositoryProbe = {
      folderId: args.folderId ? String(args.folderId) : crypto.randomUUID(),
      repositoryId: 'd'.repeat(32),
      kind: args.encrypted ? 'encrypted' : 'plain',
      state: 'repository',
      saved: !!args.remember,
    };
    demoProbes.set(p.folderId, p);
    const v = demoView(p.folderId);
    v.items = [];
    const supplied = args.unlock as RepositoryUnlock;
    v.unlockMethods = p.kind === 'encrypted' ? (supplied?.password ? ['password'] : ['key']) : [];
    demoSessions.set(v.handle, v);
    return v as T;
  }
  if (command === 'import_repository_key')
    return { selectionId: 'demo-key', name: '示例.key（演示）' } as T;
  if (command === 'generate_repository_key')
    return { selectionId: 'demo-generated-key', name: '新密钥.key（演示）' } as T;
  const view = demoSessions.get(String(args.handle));
  if (command === 'change_repository_unlock' && view) {
    view.credentialRevision = crypto.randomUUID();
    view.saved = !!args.remember;
    const method = (args.unlock as RepositoryUnlock).password ? 'password' : 'key';
    view.unlockMethods = [...new Set([...(view.unlockMethods ?? []), method])];
    return { view: { ...view }, notice: null } as T;
  }
  if (command === 'list_repository' && view) {
    const directory = String(args.directory);
    return { ...view, directory, items: demoDirectory(view.repositoryId, directory) } as T;
  }
  if (command === 'close_repository') {
    demoSessions.delete(String(args.handle));
    return undefined as T;
  }
  if (command === 'forget_repository_unlock') {
    for (const p of demoProbes.values()) if (p.repositoryId === args.repositoryId) p.saved = false;
    return undefined as T;
  }
  if (command === 'edit_repository' && view) {
    const e = args.edit as { kind: string; path: string; paths?: string[]; destination: string };
    const tree = demoTree(view.repositoryId);
    if (e.kind === 'mkdir') {
      if (tree.has(e.path)) throw new Error('该名称已经存在');
      tree.set(e.path, {
        name: e.path.split('/').pop()!,
        path: e.path,
        kind: 'directory',
        size: 0,
        conflict: false,
        protected: false,
      });
    } else {
      const roots = e.paths?.length ? e.paths : [e.path];
      const selected = [...tree.values()].filter((item) =>
        roots.some((root) => item.path === root || item.path.startsWith(`${root}/`)),
      );
      if (!selected.length) throw new Error('所选项目不存在');
      if (e.kind === 'remove') {
        for (const item of selected) tree.delete(item.path);
      } else {
        const moved = selected.map((item) => {
          const root = roots.find(
            (root) => item.path === root || item.path.startsWith(`${root}/`),
          )!;
          const target =
            e.kind === 'rename'
              ? e.destination
              : [e.destination, root.split('/').pop()].filter(Boolean).join('/');
          const path = target + item.path.slice(root.length);
          if (tree.has(path)) throw new Error('目标名称已经存在');
          return { ...item, path, name: path.split('/').pop()! };
        });
        if (e.kind !== 'copy') for (const item of selected) tree.delete(item.path);
        for (const item of moved) tree.set(item.path, item);
      }
    }
    view.revision = crypto.randomUUID();
    return undefined as T;
  }
  if (command === 'plan_repository_uploads')
    return {
      id: 'demo-plan',
      items: [
        { path: '使用说明.txt', size: 2048, sha256: 'c'.repeat(64), action: 'skip' },
        { path: '说明副本.txt', size: 2048, sha256: 'c'.repeat(64), action: 'reuse' },
        { path: '新照片.jpg', size: 3145728, sha256: 'e'.repeat(64), action: 'upload' },
        { path: '照片副本.jpg', size: 3145728, sha256: 'e'.repeat(64), action: 'batch-reuse' },
      ],
      logicalBytes: 6295552,
      sourceBytes: 6295552,
      uploadBytes: 3145728,
      uniqueUploads: 1,
    } as T;
  if (command === 'download_repository_file')
    return { id: 'demo-download', name: '使用说明.txt', size: 2048, needsExport: false } as T;
  if (command === 'export_repository_download') return true as T;
  if (command === 'execute_repository_uploads')
    return [
      demoJob('新照片.jpg', 'upload', 3145728, view?.encrypted ? 'encrypting' : 'queued'),
    ] as T;
  if (command === 'queue_repository_downloads' && view)
    return (args.paths as string[]).map((path) => {
      const item = demoTree(view.repositoryId).get(path);
      return demoJob(item?.name ?? '对象库下载', 'download', item?.size ?? 0);
    }) as T;
  if (command === 'repository_file_link' && view)
    return { url: 'https://example.invalid/preview-file', encrypted: view.encrypted } as T;
  if (command === 'import_cloud_to_repository' && view) {
    if (view.encrypted) throw new Error('加密对象库不支持直接移入或移出');
    const dest = String(args.destination ?? '');
    const tree = demoTree(view.repositoryId);
    for (const f of (args.files as CloudFile[]) ?? []) {
      const p = dest ? `${dest}/${f.name}` : f.name;
      if (tree.has(p)) throw new Error(`对象库目标位置已存在同名项目：${p}`);
      tree.set(p, {
        name: f.name,
        path: p,
        kind: f.isDir ? 'directory' : 'file',
        size: f.size,
        sha256: f.isDir ? undefined : 'f'.repeat(64),
        conflict: false,
        protected: false,
      });
    }
    view.revision = crypto.randomUUID();
    return undefined as T;
  }
  if (command === 'export_repository_to_cloud' && view) {
    if (view.encrypted) throw new Error('加密对象库不支持直接移入或移出');
    if (!args.copy) {
      const tree = demoTree(view.repositoryId);
      for (const root of (args.paths as string[]) ?? []) {
        for (const key of [...tree.keys()]) {
          if (key === root || key.startsWith(`${root}/`)) tree.delete(key);
        }
      }
      view.revision = crypto.randomUUID();
    }
    return undefined as T;
  }
  if (command === 'relocate_between_repositories') {
    const src = demoSessions.get(String(args.sourceHandle));
    const dst = demoSessions.get(String(args.targetHandle));
    if (!src || !dst || src.encrypted || dst.encrypted)
      throw new Error('仅支持普通对象库跨库流转');
    const srcTree = demoTree(src.repositoryId);
    const dstTree = demoTree(dst.repositoryId);
    const dest = String(args.destination ?? '');
    for (const root of (args.paths as string[]) ?? []) {
      const top = root.split('/').pop()!;
      const targetRoot = dest ? `${dest}/${top}` : top;
      for (const [k, v] of [...srcTree.entries()]) {
        if (k === root || k.startsWith(`${root}/`)) {
          const nextPath = k === root ? targetRoot : `${targetRoot}${k.slice(root.length)}`;
          dstTree.set(nextPath, { ...v, path: nextPath, name: nextPath.split('/').pop()! });
          if (!args.copy) srcTree.delete(k);
        }
      }
    }
    src.revision = crypto.randomUUID();
    dst.revision = crypto.randomUUID();
    return undefined as T;
  }
  throw new Error('此演示操作不可用');
}
export const repositoryApi = {
  link: (handle: string, revision: string, path: string) =>
    call<{ url: string; encrypted: boolean }>('repository_file_link', { handle, revision, path }),
  caches: () => call<RepositoryCache[]>('list_repository_caches'),
  deleteCache: (id: string) => call<void>('delete_repository_cache', { id }),
  labelCache: (handle: string, name: string) =>
    call<void>('label_repository_cache', { handle, name }),
  forgetAll: () => call<number>('forget_all_repository_credentials', { confirmed: true }),
  download: (handle: string, revision: string, path: string) =>
    call<{ id: string; name: string; size: number; needsExport: boolean } | null>(
      'download_repository_file',
      { handle, revision, path },
    ),
  exportDownload: (id: string) => call<boolean>('export_repository_download', { id }),
  probe: (folderIds: string[]) => call<RepositoryProbe[]>('probe_repositories', { folderIds }),
  open: async (
    folderId: string,
    repositoryId: string,
    unlock: RepositoryUnlock = {},
    useSaved = false,
    remember = false,
    onSession?: (handle: string) => void,
  ) => {
    const requestId = crypto.randomUUID();
    const unlisten =
      native && onSession
        ? await listen<{ requestId: string; handle: string }>(
            'repository-opening',
            ({ payload }) => {
              if (payload.requestId === requestId) onSession(payload.handle);
            },
          )
        : undefined;
    try {
      return await call<RepositoryView>('open_repository', {
        folderId,
        repositoryId,
        unlock,
        useSaved,
        remember,
        requestId,
      });
    } finally {
      unlisten?.();
    }
  },
  create: (folderId: string, encrypted: boolean, unlock: RepositoryUnlock, remember: boolean) =>
    call<RepositoryView>('create_repository', { folderId, encrypted, unlock, remember }),
  createNamed: (
    parent: CloudFile,
    name: string,
    encrypted: boolean,
    unlock: RepositoryUnlock,
    remember: boolean,
  ) =>
    call<RepositoryView>('create_named_repository', { parent, name, encrypted, unlock, remember }),
  changeUnlock: (handle: string, revision: string, unlock: RepositoryUnlock, remember: boolean) =>
    call<{ view: RepositoryView; notice: string | null }>('change_repository_unlock', {
      handle,
      revision,
      unlock,
      remember,
    }),
  list: (handle: string, directory: string, refresh = false) =>
    call<RepositoryView>('list_repository', { handle, directory, refresh }),
  close: (handle: string) => call<void>('close_repository', { handle }),
  forget: (repositoryId: string) => call<void>('forget_repository_unlock', { repositoryId }),
  importKey: () => call<{ selectionId: string; name: string } | null>('import_repository_key'),
  generateKey: () => call<{ selectionId: string; name: string } | null>('generate_repository_key'),
  edit: (
    handle: string,
    revision: string,
    kind: 'mkdir' | 'rename' | 'remove' | 'copy' | 'move',
    path: string,
    destination = '',
    recursive = false,
  ) =>
    call<void>('edit_repository', {
      handle,
      edit: { kind, path, revision, destination, recursive },
    }),
  batchEdit: (
    handle: string,
    revision: string,
    kind: 'remove' | 'copy' | 'move',
    paths: string[],
    destination = '',
  ) =>
    call<void>('edit_repository', {
      handle,
      edit: { kind, revision, path: '', paths, destination, recursive: true },
    }),
  plan: (
    handle: string,
    directory: string,
    mode: 'files' | 'directory' = 'files',
    paths?: string[],
  ) =>
    call<RepositoryUploadPlan | null>('plan_repository_uploads', {
      handle,
      directory,
      mode,
      paths,
    }),
  execute: (handle: string, planId: string, replace: boolean) =>
    call<RawJob[]>('execute_repository_uploads', { handle, planId, replace }).then((jobs) =>
      jobs.map(normalizeJob),
    ),
  queueDownloads: (handle: string, revision: string, paths: string[]) =>
    call<RawJob[]>('queue_repository_downloads', { handle, revision, paths }).then((jobs) =>
      jobs.map(normalizeJob),
    ),
  startPreview: (handle: string, revision: string, path: string) =>
    call<RepositoryPreviewStart>('preview_repository_file', { handle, revision, path }),
  previewReady: (id: string) =>
    call<{ path: string | null; status: string }>('repository_preview_ready', { id }),
  releasePreview: (id: string) => call<void>('release_repository_preview', { id }),
  openMediaPlayer: (id: string) => call<void>('open_repository_media_player', { id }),
  importFromCloud: (
    handle: string,
    revision: string | undefined,
    destination: string,
    files: CloudFile[],
    copy: boolean,
  ) =>
    call<void>('import_cloud_to_repository', {
      handle,
      revision,
      destination,
      files,
      copy,
    }),
  exportToCloud: (
    handle: string,
    revision: string,
    paths: string[],
    destination: CloudFile,
    copy: boolean,
  ) =>
    call<void>('export_repository_to_cloud', {
      handle,
      revision,
      paths,
      destination,
      copy,
    }),
  relocateBetweenRepositories: (
    sourceHandle: string,
    sourceRevision: string,
    paths: string[],
    targetHandle: string,
    targetRevision: string | undefined,
    destination: string,
    copy: boolean,
  ) =>
    call<void>('relocate_between_repositories', {
      sourceHandle,
      sourceRevision,
      paths,
      targetHandle,
      targetRevision,
      destination,
      copy,
    }),
};
