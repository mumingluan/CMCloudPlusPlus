import type { CloudFile } from './types';
export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  const power = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / 1024 ** power).toLocaleString('zh-CN', { maximumFractionDigits: power > 1 ? 2 : 0 })} ${units[power]}`;
}
export function fileKind(file: CloudFile): string {
  if (file.isDir) return '文件夹';
  const ext = file.name.split('.').pop()?.toLowerCase() ?? '';
  if (['jpg', 'jpeg', 'png', 'webp', 'gif', 'heic', 'bmp'].includes(ext)) return '图片';
  if (['mp4', 'mov', 'mkv', 'avi', 'webm'].includes(ext)) return '视频';
  if (['mp3', 'wav', 'flac', 'm4a', 'aac'].includes(ext)) return '音频';
  if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext)) return '压缩包';
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
