import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { FileDropEvent } from './useFileUploadDrop';
import type { TransferOverview } from './transferView';
import type {
  AccountSummary,
  BrowseSource,
  CloudFile,
  FilePage,
  LoginConfig,
  TransferJob,
  TransferSpec,
} from './types';

export const native = isTauri();
export const android = native && /Android/i.test(navigator.userAgent);
export const setVideoImmersive = (enabled: boolean) => invoke<void>('set_video_immersive', { enabled });
export const mobile = native && /Android|iPhone|iPad/i.test(navigator.userAgent);
export const api = {
  ready: () => call<void>('frontend_ready'),
  account: () => call<AccountSummary | null>('get_account'),
  login: (config: LoginConfig) => call<AccountSummary>('connect_account', { config }),
  accounts: () => call<AccountSummary[]>('list_accounts'),
  selectAccount: (id: string) => call<AccountSummary>('select_account', { id }),
  removeAccount: (id: string) => call<AccountSummary | null>('remove_account', { id }),
  official: () => call<Record<string, unknown>>('discover_official'),
  importOfficial: () => call<AccountSummary>('import_official'),
  browseSources: (accountId: string) => call<BrowseSource[]>('list_browse_sources', { accountId }),
  addBrowseSource: (accountId: string, kind: BrowseSource['kind'], input: string) =>
    call<BrowseSource>('add_browse_source', { accountId, kind, input }),
  removeBrowseSource: (accountId: string, id: string) =>
    call<void>('remove_browse_source', { accountId, id }),
  list: (parent: CloudFile, cursor: string | null) =>
    call<FilePage>('list_files', { parent, cursor }),
  quota: () => call<unknown>('get_quota'),
  mkdir: (parent: CloudFile, name: string) => call<void>('make_directory', { parent, name }),
  rename: (file: CloudFile, name: string) => call<void>('rename_file', { file, name }),
  remove: (files: CloudFile[]) =>
    call<{ removedIds: string[]; errors: string[] }>('remove_files', { files }),
  relocate: (files: CloudFile[], destination: CloudFile, copy: boolean) =>
    call<void>('relocate_files', { files, destination, copy }),
  link: (file: CloudFile) => call<string>('get_link', { file }),
  preview: (file: CloudFile) => call<unknown>('get_preview', { file }),
  download: (files: CloudFile[]) =>
    call<RawJob[]>('queue_downloads', { files }).then((jobs) => jobs.map(normalizeJob)),
  upload: (parent: CloudFile) =>
    call<RawJob[]>('queue_uploads', { parent }).then((jobs) => jobs.map(normalizeJob)),
  uploadDirectory: (parent: CloudFile) =>
    call<{ queued: number; skipped: number }>('queue_upload_directory', { parent }),
  uploadPaths: (parent: CloudFile, paths: string[]) =>
    call<{ queued: number; skipped: number }>('queue_upload_directory', { parent, paths }),
  cancelScan: () => call<void>('cancel_scan'),
  enqueue: (specs: TransferSpec[]) =>
    call<RawJob[]>('queue_transfers', { specs }).then((jobs) => jobs.map(normalizeJob)),
  jobs: (limit = 200, offset = 0, filter = 'all') =>
    call<RawJob[]>('list_transfers', { limit, offset, filter }).then((jobs) =>
      jobs.map(normalizeJob),
    ),
  summary: () => call<{ status: string; direction: string; count: number }[]>('transfer_summary'),
  overview: () => call<TransferOverview>('transfer_overview'),
  control: (id: string, action: string) => call<void>('control_transfer', { id, action }),
  controlBatch: (action: 'pause' | 'resume' | 'cancel', direction: 'all' | 'upload' | 'download') =>
    call<number>('control_transfers', { action, direction }),
  clear: () => call<number>('clear_finished'),
  removeCompleted: (id: string) => call<void>('remove_completed_transfer', { id }),
  settings: (concurrency: number) => call<void>('set_concurrency', { concurrency }),
  options: () =>
    call<{
      concurrency: number;
      customUploadPartSize: number;
      useLargeThumbnail: boolean;
    }>('get_options'),
  foreground: () => call<ForegroundStatus>('get_foreground_status'),
  requestTransferProtection: () => call<ForegroundStatus>('request_transfer_protection'),
  openCompleted: (id: string, reveal = false) =>
    call<void>('open_completed_transfer', { id, reveal }),
  exportCompleted: (id: string) => call<boolean>('export_completed_transfer', { id }),
  updateOptions: (config: Partial<LoginConfig>) =>
    call<AccountSummary>('update_options', { config }),
};
export interface ForegroundStatus {
  supported: boolean;
  active: boolean;
  notificationPermission: boolean;
  userStopped?: boolean;
  reason: string | null;
}
export async function watchForeground(callback: (value: ForegroundStatus) => void) {
  if (!native) return () => {};
  return listen<ForegroundStatus>('foreground-status', (event) => callback(event.payload));
}
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!native) throw new Error('请在桌面或移动应用中使用此功能。浏览器用于查看界面。');
  return invoke<T>(command, args);
}
export interface RawJob extends Omit<TransferJob, 'size' | 'completed' | 'createdAt'> {
  totalBytes: number;
  completedBytes: number;
  createdAt: number;
}
export function normalizeJob(job: RawJob): TransferJob {
  return {
    ...job,
    size: job.totalBytes,
    completed: job.completedBytes,
    createdAt: new Date(job.createdAt * 1000).toISOString(),
  };
}
export async function watchFileDrop(callback: (event: FileDropEvent) => void) {
  if (!native || mobile) return () => {};
  return getCurrentWebview().onDragDropEvent((event) => callback(event.payload));
}

export interface TransferProgress {
  id: string;
  completedBytes: number;
  totalBytes: number;
  speed: number;
  phase: string;
}
export interface DirectoryChange {
  accountId: string;
  space: string;
  cloudId: string;
  parentId: string;
  file: CloudFile | null;
}
export async function watchDirectory(callback: (change: DirectoryChange) => void) {
  if (!native) return () => {};
  return listen<DirectoryChange>('directory-changed', (event) => callback(event.payload));
}
export async function watchTransfers(
  callback: (job: TransferProgress) => void,
  changed: () => void,
) {
  if (!native) return () => {};
  const clean = await listen<TransferProgress>('transfer-progress', (event) =>
    callback(event.payload),
  );
  const cleanChanged = await listen('transfer-changed', changed);
  return () => {
    clean();
    cleanChanged();
  };
}
export async function watchScan(callback: (value: { queued: number; path: string }) => void) {
  if (!native) return () => {};
  return listen<{ queued: number; path: string }>('scan-progress', (event) =>
    callback(event.payload),
  );
}
export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
