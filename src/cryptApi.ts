import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { native, normalizeJob } from './api';
import type { RawJob } from './api';
import type { CloudFile } from './types';

export type CryptNameEncryption = 'standard' | 'obfuscate' | 'off';
export type CryptNameEncoding = 'base64' | 'base32' | 'base32768';

export interface CryptFolderOptions {
  filenameEncryption: CryptNameEncryption;
  directoryNameEncryption: boolean;
  filenameEncoding: CryptNameEncoding;
  encryptedSuffix: string;
  showHidden: boolean;
}

export const defaultCryptOptions: CryptFolderOptions = {
  filenameEncryption: 'standard',
  directoryNameEncryption: true,
  filenameEncoding: 'base64',
  encryptedSuffix: '.bin',
  showHidden: true,
};

export interface CryptFolderProbe {
  folderId: string;
  known: boolean;
  saved: boolean;
  options: CryptFolderOptions | null;
}

export interface CryptItem {
  name: string;
  path: string;
  kind: 'file' | 'directory';
  size: number;
  encryptedSize: number;
  modifiedAt: string;
}

export interface CryptView {
  handle: string;
  folderId: string;
  directory: string;
  items: CryptItem[];
  saved: boolean;
  unrecognizedCount: number;
  options: CryptFolderOptions;
}

export interface CryptPreviewStart {
  url: string | null;
  streamId?: string | null;
}

export async function watchCryptDirectory(callback: (folderId: string) => void) {
  if (!native) return () => {};
  return listen<string>('crypt-directory-changed', (event) => callback(event.payload));
}

export const cryptDemo =
  !native && new URLSearchParams(location.search).get('preview') === 'scale';

const demoProbes = new Map<string, CryptFolderProbe>([
  [
    'fixture-8',
    {
      folderId: 'fixture-8',
      known: true,
      saved: false,
      options: { ...defaultCryptOptions },
    },
  ],
]);

const demoSessions = new Map<string, CryptView>();
const demoTrees = new Map<string, Map<string, CryptItem>>();

function demoTree(folderId: string) {
  let tree = demoTrees.get(folderId);
  if (!tree) {
    const initial: CryptItem[] = [
      {
        name: '私密文档',
        path: '私密文档',
        kind: 'directory',
        size: 0,
        encryptedSize: 0,
        modifiedAt: '2026-11-20T09:00:00+08:00',
      },
      {
        name: '备份清单.md',
        path: '备份清单.md',
        kind: 'file',
        size: 4096,
        encryptedSize: 4144,
        modifiedAt: '2026-11-20T10:30:00+08:00',
      },
      {
        name: '归档合同.pdf',
        path: '私密文档/归档合同.pdf',
        kind: 'file',
        size: 131072,
        encryptedSize: 131136,
        modifiedAt: '2026-11-20T11:15:00+08:00',
      },
    ];
    tree = new Map(initial.map((item) => [item.path, { ...item }]));
    demoTrees.set(folderId, tree);
  }
  return tree;
}

function demoDirectory(folderId: string, directory: string): CryptItem[] {
  return [...demoTree(folderId).values()]
    .filter((item) => item.path.split('/').slice(0, -1).join('/') === directory)
    .map((item) => ({ ...item }));
}

function demoJob(
  name: string,
  direction: 'upload' | 'download',
  bytes: number,
  status = direction === 'upload' ? 'encrypting' : 'decrypting',
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
  if (!cryptDemo) throw new Error('请在桌面或移动应用中使用加密文件夹。');

  if (command === 'probe_crypt_folders') {
    return (args.folderIds as string[]).map(
      (folderId) =>
        demoProbes.get(folderId) ?? {
          folderId,
          known: false,
          saved: false,
          options: null,
        },
    ) as T;
  }
  if (command === 'create_crypt_folder') {
    const folderId = crypto.randomUUID();
    const options = (args.options as CryptFolderOptions) ?? { ...defaultCryptOptions };
    const saved = !!args.remember;
    demoProbes.set(folderId, { folderId, known: true, saved, options: { ...options } });
    demoTrees.set(folderId, new Map());
    const view: CryptView = {
      handle: crypto.randomUUID(),
      folderId,
      directory: '',
      items: [],
      saved,
      unrecognizedCount: 0,
      options: { ...options },
    };
    demoSessions.set(view.handle, view);
    return view as T;
  }
  if (command === 'open_crypt_folder') {
    const folder = args.folder as CloudFile;
    const probe = demoProbes.get(folder.id);
    if (!args.useSaved && String(args.password ?? '') !== 'demo') {
      throw new Error('演示口令为 demo');
    }
    const options =
      (args.options as CryptFolderOptions | undefined) ??
      probe?.options ?? { ...defaultCryptOptions };
    const saved = !!args.remember || !!probe?.saved;
    demoProbes.set(folder.id, {
      folderId: folder.id,
      known: true,
      saved,
      options: { ...options },
    });
    const view: CryptView = {
      handle: crypto.randomUUID(),
      folderId: folder.id,
      directory: '',
      items: demoDirectory(folder.id, ''),
      saved,
      unrecognizedCount: 0,
      options: { ...options },
    };
    demoSessions.set(view.handle, view);
    return view as T;
  }
  if (command === 'close_crypt_folder') {
    demoSessions.delete(String(args.handle));
    return undefined as T;
  }
  if (command === 'update_crypt_folder_settings') {
    const folderId = String(args.folderId);
    if (args.forget) {
      demoProbes.delete(folderId);
      for (const [handle, session] of demoSessions) {
        if (session.folderId === folderId) demoSessions.delete(handle);
      }
      return { folderId, known: false, saved: false, options: null } as T;
    }
    const prev = demoProbes.get(folderId);
    const options =
      (args.options as CryptFolderOptions | undefined) ??
      prev?.options ?? { ...defaultCryptOptions };
    const saved = !!args.remember;
    const next: CryptFolderProbe = { folderId, known: true, saved, options: { ...options } };
    demoProbes.set(folderId, next);
    return next as T;
  }
  const view = demoSessions.get(String(args.handle));
  if (command === 'list_crypt_folder' && view) {
    const directory = String(args.directory ?? '');
    const next = {
      ...view,
      directory,
      items: demoDirectory(view.folderId, directory),
    };
    demoSessions.set(view.handle, next);
    return next as T;
  }
  if (command === 'edit_crypt_folder' && view) {
    const tree = demoTree(view.folderId);
    const action = String(args.action);
    const path = String(args.path);
    const parentDir = path.split('/').slice(0, -1).join('/');
    if (action === 'mkdir') {
      if (tree.has(path)) throw new Error('同名文件夹已存在');
      tree.set(path, {
        name: path.split('/').pop()!,
        path,
        kind: 'directory',
        size: 0,
        encryptedSize: 0,
        modifiedAt: new Date().toISOString(),
      });
    } else if (action === 'rename') {
      const target = String(args.target);
      const selected = [...tree.values()].filter(
        (item) => item.path === path || item.path.startsWith(`${path}/`),
      );
      for (const item of selected) tree.delete(item.path);
      for (const item of selected) {
        const nextPath = target + item.path.slice(path.length);
        tree.set(nextPath, { ...item, path: nextPath, name: nextPath.split('/').pop()! });
      }
    }
    const next = { ...view, directory: parentDir, items: demoDirectory(view.folderId, parentDir) };
    demoSessions.set(view.handle, next);
    return next as T;
  }
  if (command === 'batch_edit_crypt_folder' && view) {
    const tree = demoTree(view.folderId);
    const action = String(args.action);
    const paths = args.paths as string[];
    const dest = String(args.destination ?? '');
    const currentDir = (paths[0] ?? '').split('/').slice(0, -1).join('/');
    const selected = [...tree.values()].filter((item) =>
      paths.some((root) => item.path === root || item.path.startsWith(`${root}/`)),
    );
    if (action === 'remove') {
      for (const item of selected) tree.delete(item.path);
    } else {
      const moved = selected.map((item) => {
        const root = paths.find((r) => item.path === r || item.path.startsWith(`${r}/`))!;
        const base = [dest, root.split('/').pop()].filter(Boolean).join('/');
        const nextPath = base + item.path.slice(root.length);
        return { ...item, path: nextPath, name: nextPath.split('/').pop()! };
      });
      if (action === 'move') for (const item of selected) tree.delete(item.path);
      for (const item of moved) tree.set(item.path, item);
    }
    const next = {
      ...view,
      directory: currentDir,
      items: demoDirectory(view.folderId, currentDir),
    };
    demoSessions.set(view.handle, next);
    return next as T;
  }
  if (command === 'queue_crypt_uploads') {
    return [demoJob('加密文件.pdf', 'upload', 1048576)] as T;
  }
  if (command === 'queue_crypt_upload_directory') {
    return { queued: 2, skipped: 0 } as T;
  }
  if (command === 'queue_crypt_downloads' && view) {
    const tree = demoTree(view.folderId);
    return (args.paths as string[]).map((path) => {
      const item = tree.get(path);
      return demoJob(item?.name ?? '解密下载', 'download', item?.size ?? 0);
    }) as T;
  }
  if (command === 'preview_crypt_file') {
    return { url: 'https://example.invalid/crypt-preview', streamId: 'demo-crypt-stream' } as T;
  }
  if (command === 'release_crypt_preview') return true as T;
  if (command === 'open_crypt_media_player') return undefined as T;
  throw new Error('此演示操作不可用');
}

export const cryptApi = {
  probe: (folderIds: string[]) => call<CryptFolderProbe[]>('probe_crypt_folders', { folderIds }),
  create: (
    parent: CloudFile,
    name: string,
    password: string,
    salt: string,
    options: CryptFolderOptions,
    remember: boolean,
  ) =>
    call<CryptView>('create_crypt_folder', {
      parent,
      name,
      password,
      salt,
      options,
      remember,
    }),
  open: (
    folder: CloudFile,
    params: {
      password?: string;
      salt?: string;
      options?: CryptFolderOptions;
      useSaved?: boolean;
      remember?: boolean;
    } = {},
  ) =>
    call<CryptView>('open_crypt_folder', {
      folder,
      password: params.password ?? null,
      salt: params.salt ?? null,
      options: params.options ?? null,
      useSaved: !!params.useSaved,
      remember: !!params.remember,
    }),
  close: (handle: string) => call<void>('close_crypt_folder', { handle }),
  list: (handle: string, directory: string, force = false) =>
    call<CryptView>('list_crypt_folder', { handle, directory, force }),
  edit: (handle: string, action: 'mkdir' | 'rename', path: string, target?: string) =>
    call<CryptView>('edit_crypt_folder', { handle, action, path, target: target ?? null }),
  batchEdit: (
    handle: string,
    action: 'remove' | 'move' | 'copy',
    paths: string[],
    destination?: string,
  ) =>
    call<CryptView>('batch_edit_crypt_folder', {
      handle,
      action,
      paths,
      destination: destination ?? null,
    }),
  updateSettings: (
    folderId: string,
    params: {
      password?: string;
      salt?: string;
      options?: CryptFolderOptions;
      remember: boolean;
      forget?: boolean;
    },
  ) =>
    call<CryptFolderProbe>('update_crypt_folder_settings', {
      folderId,
      password: params.password ?? null,
      salt: params.salt ?? null,
      options: params.options ?? null,
      remember: params.remember,
      forget: !!params.forget,
    }),
  queueUploads: async (handle: string, directory: string) =>
    (await call<RawJob[]>('queue_crypt_uploads', { handle, directory })).map(normalizeJob),
  queueUploadDirectory: (handle: string, directory: string, paths?: string[]) =>
    call<{ queued: number; skipped: number }>('queue_crypt_upload_directory', {
      handle,
      directory,
      paths: paths ?? null,
    }),
  queueDownloads: async (handle: string, paths: string[]) =>
    (await call<RawJob[]>('queue_crypt_downloads', { handle, paths })).map(normalizeJob),
  startPreview: (handle: string, path: string) =>
    call<CryptPreviewStart>('preview_crypt_file', { handle, path }),
  releasePreview: (id: string) => call<boolean>('release_crypt_preview', { id }),
  openMediaPlayer: (id: string) => call<void>('open_crypt_media_player', { id }),
};
