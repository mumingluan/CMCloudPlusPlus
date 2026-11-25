import type { CloudFile, FilePage } from './types';
import { cryptApi } from './cryptApi';
import type { CryptItem, CryptView } from './cryptApi';

export function cryptFile(folderId: string, item: CryptItem): CloudFile {
  return {
    id: `crypt:${folderId}:${item.path}`,
    name: item.name,
    path: item.path,
    isDir: item.kind === 'directory',
    size: item.size,
    modifiedAt: item.modifiedAt,
  };
}

export function cryptTrail(folderId: string, directory: string, name: string): CloudFile[] {
  const item = (path: string, label: string) =>
    cryptFile(folderId, {
      name: label,
      path,
      kind: 'directory',
      size: 0,
      encryptedSize: 0,
      modifiedAt: '',
    });
  const parts = directory.split('/').filter(Boolean);
  return [
    item('', name),
    ...parts.map((part, index) => item(parts.slice(0, index + 1).join('/'), part)),
  ];
}

export function cryptOperations(view: CryptView, onView?: (next: CryptView) => void) {
  const join = (directory: string, name: string) => (directory ? `${directory}/${name}` : name);
  const syncIfCurrent = (next: CryptView) => {
    if (onView && next.directory === view.directory) onView(next);
  };
  return {
    list: async (parent: CloudFile, _cursor: string | null): Promise<FilePage> => {
      const next = await cryptApi.list(view.handle, parent.path, false);
      return {
        files: next.items.map((item) => cryptFile(next.folderId, item)),
        nextCursor: null,
      };
    },
    mkdir: async (parent: CloudFile, name: string) => {
      const next = await cryptApi.edit(view.handle, 'mkdir', join(parent.path, name));
      syncIfCurrent(next);
    },
    rename: async (file: CloudFile, name: string) => {
      const parentDir = file.path.split('/').slice(0, -1).join('/');
      const next = await cryptApi.edit(view.handle, 'rename', file.path, join(parentDir, name));
      syncIfCurrent(next);
    },
    remove: async (files: CloudFile[]) => {
      const next = await cryptApi.batchEdit(
        view.handle,
        'remove',
        files.map((file) => file.path),
      );
      syncIfCurrent(next);
      return { removedIds: files.map((file) => file.id), errors: [] as string[] };
    },
    relocate: async (files: CloudFile[], destination: CloudFile, copy: boolean) => {
      const next = await cryptApi.batchEdit(
        view.handle,
        copy ? 'copy' : 'move',
        files.map((file) => file.path),
        destination.path,
      );
      syncIfCurrent(next);
    },
  };
}
