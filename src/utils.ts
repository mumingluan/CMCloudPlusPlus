import type { CloudFile } from './types';
import mediaFormats from './mediaFormats.json';
export const vendorFolders = new Set(['手机图片', '同步盘', '我的应用收藏', 'AI空间']);
export function isVendorFolder(file: CloudFile, atRoot: boolean) {
  return atRoot && file.isDir && vendorFolders.has(file.name);
}
export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const power = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / 1024 ** power).toLocaleString('zh-CN', { maximumFractionDigits: power > 1 ? 2 : 0 })} ${units[power]}`;
}
export function mediaMime(name: string) {
  const ext = name.includes('.') ? name.split('.').pop()!.toLowerCase() : '';
  return Object.hasOwn(mediaFormats, ext) ? mediaFormats[ext as keyof typeof mediaFormats] : undefined;
}
export function fileKind(file: CloudFile): string {
  if (file.isDir) return '文件夹';
  const ext = file.name.split('.').pop()?.toLowerCase() ?? '';
  if (['jpg', 'jpeg', 'png', 'webp', 'gif', 'heic', 'bmp'].includes(ext)) return '图片';
  const media = mediaMime(file.name);
  if (media?.startsWith('video/')) return '视频';
  if (media?.startsWith('audio/')) return '音频';
  if (['zip', 'rar', '7z', 'tar', 'gz', 'tgz'].includes(ext)) return '压缩包';
  return ext.toUpperCase() || '文件';
}
export function sortFiles(files: CloudFile[], key: string, desc: boolean): CloudFile[] {
  const collator = new Intl.Collator('zh-CN', { numeric: true });
  return [...files].sort(
    (a, b) =>
      Number(b.isDir) - Number(a.isDir) ||
      (desc ? -1 : 1) *
        (key === 'size'
          ? a.size - b.size
          : key === 'modifiedAt'
            ? a.modifiedAt.localeCompare(b.modifiedAt)
            : collator.compare(a.name, b.name)),
  );
}
export function validName(name: string): boolean {
  return !!name.trim() && !/[\x00-\x1f/\\]/.test(name) && !['.', '..'].includes(name.trim());
}
export function formatCloudPath(segments: Pick<CloudFile, 'name'>[]): string {
  const names = segments.map((item) => item.name).filter(Boolean);
  return '/' + names.join('/');
}
export function formatRepositoryPath(cloudPath: string, innerPath: string): string {
  const base = cloudPath.startsWith('/') ? cloudPath : `/${cloudPath}`;
  return `${base}:${innerPath}`;
}
export function normalizePathSegments(parts: string[]): string[] {
  const out: string[] = [];
  for (const raw of parts) {
    const part = raw.trim();
    if (!part || part === '.') continue;
    if (part === '..') out.pop();
    else out.push(part);
  }
  return out;
}
export type ParsedAddressPath =
  | { kind: 'cloud'; segments: string[] }
  | { kind: 'repository'; cloudSegments: string[]; innerPath: string };
export function parseAddressPath(
  raw: string,
  context?: {
    cloudSegments?: string[];
    repository?: { cloudSegments: string[]; innerPath: string };
  },
): ParsedAddressPath {
  const trimmed = raw.trim().replace(/\\+/g, '/');
  if (!trimmed || trimmed === '/') return { kind: 'cloud', segments: [] };
  const colon = trimmed.indexOf(':');
  if (colon >= 0) {
    const cloudPart = trimmed.slice(0, colon).trim();
    const innerPart = trimmed.slice(colon + 1).trim();
    const cloudSegments = cloudPart
      ? normalizePathSegments([
          ...(cloudPart.startsWith('/') ? [] : (context?.cloudSegments ?? [])),
          ...cloudPart.split('/'),
        ])
      : (context?.repository?.cloudSegments ?? []);
    const innerPath = normalizePathSegments(innerPart.split('/')).join('/');
    return { kind: 'repository', cloudSegments, innerPath };
  }
  if (!trimmed.startsWith('/') && context?.repository) {
    const base = context.repository.innerPath ? context.repository.innerPath.split('/') : [];
    return {
      kind: 'repository',
      cloudSegments: context.repository.cloudSegments,
      innerPath: normalizePathSegments([...base, ...trimmed.split('/')]).join('/'),
    };
  }
  const base = trimmed.startsWith('/') ? [] : (context?.cloudSegments ?? []);
  return {
    kind: 'cloud',
    segments: normalizePathSegments([...base, ...trimmed.split('/')]),
  };
}
export function previewSources(value: unknown): { url: string; label: string }[] {
  const result: { url: string; label: string }[] = [];
  const visit = (entry: unknown, key: string, depth: number) => {
    if (depth > 8 || result.length >= 30) return;
    if (
      typeof entry === 'string' &&
      /url|play|src/i.test(key) &&
      !/image|thumb|cover|poster/i.test(key)
    ) {
      try {
        const url = new URL(entry);
        if (url.protocol === 'https:' || url.protocol === 'http:') {
          if (!result.some((item) => item.url === entry))
            result.push({ url: entry, label: `播放源 ${result.length + 1}` });
        }
      } catch {
        /* Provider metadata may contain relative or non-media strings. */
      }
    } else if (Array.isArray(entry))
      entry.slice(0, 100).forEach((item) => visit(item, key, depth + 1));
    else if (entry && typeof entry === 'object')
      Object.entries(entry)
        .slice(0, 100)
        .forEach(([key, item]) => visit(item, key, depth + 1));
  };
  visit(value, 'url', 0);
  return result;
}
export function dateText(value: string): string {
  if (!value) return '—';
  const normalized = /^\d{14}$/.test(value)
    ? `${value.slice(0, 4)}-${value.slice(4, 6)}-${value.slice(6, 8)}T${value.slice(8, 10)}:${value.slice(10, 12)}:${value.slice(12, 14)}+08:00`
    : value;
  const parsed = new Date(normalized);
  return Number.isNaN(parsed.getTime())
    ? value
    : parsed.toLocaleString('zh-CN', {
        hour12: false,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      });
}
