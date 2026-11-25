import { useCallback, useEffect, useRef, useState } from 'react';
import { repositoryApi } from './repositoryApi';
import type { RepositoryProbe } from './repositoryApi';

const emptyProbes: Record<string, RepositoryProbe> = {};

export function useRepositoryDiscovery(
  folderIds: string[],
  scope: string,
  enabled: boolean,
  revision: number,
) {
  const [cache, setCache] = useState({ scope, probes: emptyProbes });
  const seen = useRef({ scope, revision, ids: new Set<string>() });
  const key = folderIds.slice(0, 32).join('\0');
  const remember = useCallback(
    (rows: RepositoryProbe[]) => {
      setCache((previous) => {
        const probes = previous.scope === scope ? { ...previous.probes } : {};
        for (const row of rows) {
          if (row.state !== 'unavailable' || probes[row.folderId]?.state !== 'repository') {
            probes[row.folderId] = row;
          }
        }
        return { scope, probes };
      });
    },
    [scope],
  );
  useEffect(() => {
    // A credential change schedules fresh probes without dropping confirmed folders.
    if (seen.current.scope !== scope || seen.current.revision !== revision) {
      seen.current = { scope, revision, ids: new Set() };
    }
    if (!enabled || !key) return;
    let disposed = false;
    const checked = seen.current.ids;
    const ids = key.split('\0').filter((id) => !checked.has(id));
    if (!ids.length) return;
    ids.forEach((id) => checked.add(id));
    let finished = false;
    void repositoryApi
      .probe(ids)
      .then((rows) => {
        finished = true;
        if (!disposed) remember(rows);
      })
      .catch(() => {
        finished = true;
        if (!disposed) ids.forEach((id) => checked.delete(id));
      });
    return () => {
      disposed = true;
      if (!finished) ids.forEach((id) => checked.delete(id));
    };
  }, [scope, revision, key, enabled, remember]);
  return [enabled && cache.scope === scope ? cache.probes : emptyProbes, remember] as const;
}
