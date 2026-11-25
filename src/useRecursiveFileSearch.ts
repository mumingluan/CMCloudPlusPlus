import { useEffect, useRef, useState } from 'react';
import type { CloudFile, FilePage } from './types';

type Listing = (parent: CloudFile, cursor: string | null) => Promise<FilePage>;
export interface SearchResult {
  files: CloudFile[];
  parents: Map<string, CloudFile[]>;
  scanned: number;
}

export async function searchFolderTree(
  list: Listing,
  root: CloudFile,
  query: string,
  signal: AbortSignal,
  progress: (result: SearchResult) => void,
  include: (file: CloudFile, atRoot: boolean) => boolean = () => true,
) {
  const queue = [{ folder: root, parents: [] as CloudFile[] }];
  const visited = new Set<string>();
  const matches = new Map<string, CloudFile>();
  const parents = new Map<string, CloudFile[]>();
  const needle = query.toLocaleLowerCase();
  let scanned = 0;
  for (let index = 0; index < queue.length; index++) {
    signal.throwIfAborted();
    const entry = queue[index];
    if (visited.has(entry.folder.id)) continue;
    visited.add(entry.folder.id);
    const cursors = new Set<string>();
    let cursor: string | null = null;
    do {
      signal.throwIfAborted();
      const page = await list(entry.folder, cursor);
      signal.throwIfAborted();
      for (const file of page.files) {
        if (!include(file, index === 0)) continue;
        scanned++;
        if (file.name.toLocaleLowerCase().includes(needle)) {
          matches.set(file.id, file);
          parents.set(file.id, entry.parents);
        }
        if (file.isDir && !visited.has(file.id))
          queue.push({ folder: file, parents: [...entry.parents, file] });
      }
      progress({ files: [...matches.values()], parents: new Map(parents), scanned });
      cursor = page.nextCursor ?? null;
      if (cursor && cursors.has(cursor)) throw new Error('目录分页没有前进，请重试递归搜索');
      if (cursor) cursors.add(cursor);
    } while (cursor);
  }
}

export function useRecursiveFileSearch(list: Listing, scope: string, query: string) {
  const controller = useRef<AbortController | null>(null);
  const [state, setState] = useState<(SearchResult & { running: boolean; error: string }) | null>(
    null,
  );
  const clear = () => {
    controller.current?.abort();
    controller.current = null;
    setState(null);
  };
  useEffect(() => {
    clear();
    return () => controller.current?.abort();
  }, [scope, query]);
  return {
    state,
    clear,
    cancel: () => {
      controller.current?.abort();
      controller.current = null;
      setState((previous) => previous && { ...previous, running: false });
    },
    async start(root: CloudFile, include: (file: CloudFile, atRoot: boolean) => boolean) {
      controller.current?.abort();
      const current = new AbortController();
      controller.current = current;
      setState({ files: [], parents: new Map(), scanned: 0, running: true, error: '' });
      try {
        await searchFolderTree(
          list,
          root,
          query,
          current.signal,
          (result) => {
            if (controller.current === current) setState({ ...result, running: true, error: '' });
          },
          include,
        );
      } catch (error) {
        if (!current.signal.aborted && controller.current === current)
          setState((previous) => previous && { ...previous, error: String(error) });
      } finally {
        if (controller.current === current)
          setState((previous) => previous && { ...previous, running: false });
      }
    },
  };
}
