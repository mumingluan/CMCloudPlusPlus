// @vitest-environment jsdom
import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { searchFolderTree, useRecursiveFileSearch } from './useRecursiveFileSearch';
import type { SearchResult } from './useRecursiveFileSearch';
import type { CloudFile, FilePage } from './types';

afterEach(cleanup);
const file = (id: string, isDir = false): CloudFile => ({
  id,
  name: id,
  path: `/${id}`,
  isDir,
  size: 1,
  modifiedAt: '',
});

it('searches every page and child directory while preserving the path to matches', async () => {
  const root = file('root', true),
    child = file('child', true),
    nested = file('nested', true);
  const list = vi.fn(async (dir: CloudFile, cursor: string | null): Promise<FilePage> => {
    if (dir.id === 'root')
      return cursor ? { files: [file('needle-root')] } : { files: [child], nextCursor: '2' };
    if (dir.id === 'child') return { files: [nested, file('NEEDLE-child')] };
    return { files: [root, file('needle-deep')] };
  });
  let result: SearchResult | undefined;
  await searchFolderTree(list, root, 'needle', new AbortController().signal, (value) => {
    result = value;
  });
  expect(list).toHaveBeenCalledTimes(4);
  expect(result!.files.map((item) => item.id)).toEqual([
    'needle-root',
    'NEEDLE-child',
    'needle-deep',
  ]);
  expect(result!.parents.get('needle-deep')).toEqual([child, nested]);
});

it('rejects a repeated pagination cursor instead of looping forever', async () => {
  const list = vi.fn(async () => ({ files: [], nextCursor: 'same' }));
  await expect(
    searchFolderTree(list, file('root', true), 'q', new AbortController().signal, () => {}),
  ).rejects.toThrow('分页没有前进');
  expect(list).toHaveBeenCalledTimes(2);
});

it('does not visit excluded folders and ignores in-flight results after a scope change', async () => {
  const hidden = file('hidden', true);
  const list = vi.fn(async () => ({ files: [hidden] }));
  await searchFolderTree(
    list,
    file('root', true),
    'hidden',
    new AbortController().signal,
    () => {},
    (entry) => entry.id !== 'hidden',
  );
  expect(list).toHaveBeenCalledTimes(1);
  let resolve!: (page: FilePage) => void;
  const deferred = vi.fn(
    () =>
      new Promise<FilePage>((done) => {
        resolve = done;
      }),
  );
  const { result, rerender } = renderHook(
    ({ scope }) => useRecursiveFileSearch(deferred, scope, 'needle'),
    { initialProps: { scope: '/' } },
  );
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.start(file('root', true), () => true);
  });
  rerender({ scope: '/other' });
  await act(async () => {
    resolve({ files: [file('needle')] });
    await pending;
  });
  expect(result.current.state).toBeNull();
});
