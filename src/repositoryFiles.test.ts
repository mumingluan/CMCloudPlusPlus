import { describe, expect, it, vi } from 'vitest';
import { repositoryFile, repositoryOperations, repositoryTrail } from './repositoryFiles';
import type { RepositoryView } from './repositoryApi';
const calls = vi.hoisted(() => ({ list: vi.fn(), edit: vi.fn(), batchEdit: vi.fn() }));
vi.mock('./repositoryApi', () => ({ repositoryApi: calls }));
const view: RepositoryView = {
  handle: 'private-session', repositoryId: 'a'.repeat(32), folderId: 'provider-folder',
  revision: 'verified-revision', encrypted: true, directory: '照片', items: [],
  conflicts: [], historical: false, saved: false,
};
const file = repositoryFile(view.repositoryId, { name: '照片.jpg', path: '照片/照片.jpg', kind: 'file', size: 100, modifiedAt: 1794326400000, conflict: false, protected: false });
describe('shared file workspace repository adapter', () => {
  it('keeps logical names and library identity separate from provider IDs', () => {
    expect(file.id).toBe(`repository:${view.repositoryId}:照片/照片.jpg`);
    expect(file.path).toBe('照片/照片.jpg');
    expect(file.modifiedAt).toBe('2026-11-10T16:00:00.000Z');
    const trail = repositoryTrail(view.repositoryId, '照片/旅行', '加密归档');
    expect(trail.map(row => row.path)).toEqual(['', '照片', '照片/旅行']);
    expect(trail[0].name).toBe('加密归档');
    expect(trail[0].modifiedAt).toBe('');
  });
  it('routes batch copy, move and removal to one revision-bound SDK operation', async () => {
    const ops = repositoryOperations(view);
    const root = repositoryTrail(view.repositoryId, '', '归档')[0];
    await ops.relocate([file], root, true);
    expect(calls.batchEdit).toHaveBeenLastCalledWith(view.handle, view.revision, 'copy', [file.path], '');
    await ops.relocate([file], root, false);
    expect(calls.batchEdit).toHaveBeenLastCalledWith(view.handle, view.revision, 'move', [file.path], '');
    expect(await ops.remove([file])).toEqual({ removedIds: [file.id], errors: [] });
    expect(calls.batchEdit).toHaveBeenLastCalledWith(view.handle, view.revision, 'remove', [file.path]);
  });
  it('uses the same name and destination rules for ordinary file dialogs', async () => {
    const ops = repositoryOperations(view);
    const parent = repositoryTrail(view.repositoryId, '照片', '归档')[1];
    await ops.mkdir(parent, '旅行');
    expect(calls.edit).toHaveBeenLastCalledWith(view.handle, view.revision, 'mkdir', '照片/旅行');
    await ops.rename(file, '副本.jpg');
    expect(calls.edit).toHaveBeenLastCalledWith(view.handle, view.revision, 'rename', file.path, '照片/副本.jpg');
  });
});
