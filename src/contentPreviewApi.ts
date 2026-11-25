import { invoke } from '@tauri-apps/api/core';
import { native } from './api';
import type { CloudFile } from './types';

export const textPreviewLimit = 512 * 1024;
export const pdfPreviewLimit = 64 * 1024 * 1024;
export const officePreviewLimit = 24 * 1024 * 1024;

export type ContentKind = 'archive' | 'text' | 'markdown' | 'pdf' | 'office';
export type OfficeParts = { xml: Record<string, string>; images: Record<string, string> };
export type PreviewSource =
  | { kind: 'cloud'; file: CloudFile }
  | { kind: 'repository'; handle: string; revision: string; path: string }
  | { kind: 'crypt'; handle: string; path: string };

export interface ArchiveEntry {
  id: number;
  path: string;
  size: number | null;
  directory: boolean;
  downloadable: boolean;
}

export interface ArchiveView {
  id: string;
  format: string;
  entries: ArchiveEntry[];
}

const textSuffixes = new Set([
  'txt', 'log', 'ini', 'cfg', 'conf', 'toml', 'yaml', 'yml', 'json', 'jsonl',
  'xml', 'csv', 'tsv', 'html', 'css', 'js', 'ts', 'jsx', 'tsx', 'py', 'go',
  'rs', 'java', 'c', 'h', 'cpp', 'sh', 'ps1', 'lua', 'sql',
]);

export function contentKind(file: CloudFile): ContentKind | null {
  if (file.isDir) return null;
  const name = file.name.toLowerCase();
  const suffix = name.split('.').pop() ?? '';
  if (['zip', '7z', 'rar', 'tar', 'gz', 'tgz'].includes(suffix)) return 'archive';
  if (suffix === 'pdf') return 'pdf';
  if (['docx', 'docm', 'xlsx', 'xlsm', 'pptx', 'pptm'].includes(suffix)) return 'office';
  if (['md', 'markdown', 'mdown'].includes(suffix)) return 'markdown';
  if (textSuffixes.has(suffix) || ['readme', 'license', 'makefile'].includes(name)) return 'text';
  return null;
}

function fixture() {
  return !native && new URLSearchParams(location.search).get('preview') === 'scale';
}

export const contentPreviewApi = {
  open: (source: PreviewSource): Promise<ArchiveView> =>
    fixture()
      ? Promise.resolve({
          id: 'archive-fixture',
          format: 'ZIP',
          entries: [
            { id: 0, path: '说明.md', size: 1829, directory: false, downloadable: true },
            { id: 1, path: '项目文档/安装指南.txt', size: 8274, directory: false, downloadable: true },
            { id: 2, path: '项目文档/配置参考.json', size: 17122, directory: false, downloadable: true },
            { id: 3, path: '影像/活动记录.mp4', size: 82461722, directory: false, downloadable: true },
            { id: 4, path: '影像/封面.png', size: 1392640, directory: false, downloadable: true },
          ],
        })
      : invoke('open_archive_preview', { source }),
  close: (id: string) =>
    fixture() ? Promise.resolve() : invoke<void>('close_archive_preview', { id }),
  save: (id: string, member: number) =>
    fixture()
      ? Promise.reject(new Error('示例数据仅用于界面预览。'))
      : invoke<boolean>('save_archive_member', { id, member }),
  saveSelection: (id: string, paths: string[]) =>
    fixture()
      ? Promise.reject(new Error('示例数据仅用于界面预览。'))
      : invoke<boolean>('save_archive_selection', { id, paths }),
  pdf: (source: PreviewSource) => invoke<ArrayBuffer>('read_pdf_preview', { source }),
  office: (source: PreviewSource) => invoke<OfficeParts>('read_office_preview', { source }),
  text: (source: PreviewSource) =>
    fixture()
      ? Promise.resolve('# 项目说明\n\n这是文件预览的示例内容。\n\n## 开始使用\n\n- 检查配置文件\n- 启动应用并验证连接\n\n```json\n{"mode": "preview", "enabled": true}\n```')
      : invoke<string>('read_text_preview', { source }),
};
