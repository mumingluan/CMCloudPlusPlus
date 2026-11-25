import type { CloudFile, FilePage } from './types';
import { repositoryApi } from './repositoryApi';
import type { RepositoryItem, RepositoryView } from './repositoryApi';

export function repositoryFile(repositoryId: string, item: RepositoryItem): CloudFile {
  return {
    id: `repository:${repositoryId}:${item.path}`,
    name: item.name,
    path: item.path,
    isDir: item.kind === 'directory',
    size: item.size,
    modifiedAt: item.modifiedAt == null ? '' : new Date(item.modifiedAt).toISOString(),
  };
}

export function repositoryTrail(
  repositoryId: string,
  directory: string,
  name: string,
): CloudFile[] {
  const item = (path: string, label: string) =>
    repositoryFile(repositoryId, {
      name: label,
      path,
      kind: 'directory',
      size: 0,
      conflict: false,
      protected: false,
    });
  const parts = directory.split('/').filter(Boolean);
  return [
    item('', name),
    ...parts.map((part, index) => item(parts.slice(0, index + 1).join('/'), part)),
  ];
}

export function repositoryOperations(view: RepositoryView) {
  const join = (directory: string, name: string) => (directory ? `${directory}/${name}` : name);
  return {
    list: async (parent: CloudFile, _cursor: string | null): Promise<FilePage> => {
      const next = await repositoryApi.list(view.handle, parent.path);
      return {
        files: next.items.map((item) => repositoryFile(next.repositoryId, item)),
        nextCursor: null,
      };
    },
    mkdir: (parent: CloudFile, name: string) =>
      repositoryApi.edit(view.handle, view.revision, 'mkdir', join(parent.path, name)),
    rename: (file: CloudFile, name: string) =>
      repositoryApi.edit(
        view.handle,
        view.revision,
        'rename',
        file.path,
        join(file.path.split('/').slice(0, -1).join('/'), name),
      ),
    remove: async (files: CloudFile[]) => {
      await repositoryApi.batchEdit(
        view.handle,
        view.revision,
        'remove',
        files.map((file) => file.path),
      );
      return { removedIds: files.map((file) => file.id), errors: [] as string[] };
    },
    relocate: (files: CloudFile[], destination: CloudFile, copy: boolean) =>
      repositoryApi.batchEdit(
        view.handle,
        view.revision,
        copy ? 'copy' : 'move',
        files.map((file) => file.path),
        destination.path,
      ),
  };
}
