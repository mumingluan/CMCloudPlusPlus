import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  AccountSummary,
  CloudFile,
  FilePage,
  LoginConfig,
  TransferJob,
  TransferSpec,
  Space,
} from './types';

export const native = isTauri();
export const android = native && /Android/i.test(navigator.userAgent);
export const mobile = native && /Android|iPhone|iPad/i.test(navigator.userAgent);
export const api = {
  account: () => call<AccountSummary | null>('get_account'),
  login: (config: LoginConfig) => call<AccountSummary>('connect_account', { config }),
  logout: () => call<void>('disconnect_account'),
  accounts: () => call<AccountSummary[]>('list_accounts'),
  selectAccount: (id: string) => call<AccountSummary>('select_account', { id }),
  removeAccount: (id: string) => call<void>('remove_account', { id }),
  setSkipHash: (skip: boolean) => call<void>('set_skip_upload_hash', { skip }),
  official: () => call<Record<string, unknown>>('discover_official'),
  importOfficial: () => call<AccountSummary>('import_official'),
  switchSpace: (space: Space, cloudId: string, linkId: string) =>
    call<AccountSummary>('switch_space', { space, cloudId, linkId }),
  list: (parent: CloudFile, cursor: string | null) =>
    call<FilePage>('list_files', { parent, cursor }),
  quota: () => call<unknown>('get_quota'),
  spaces: () => call<unknown>('get_spaces'),
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
  upload: (parent: CloudFile, skipHash = false) =>
    call<RawJob[]>('queue_uploads', { parent, skipHash }).then((jobs) => jobs.map(normalizeJob)),
  uploadDirectory: (parent: CloudFile, skipHash = false) =>
    call<{ queued: number; skipped: number }>('queue_upload_directory', { parent, skipHash }),
  cancelScan: () => call<void>('cancel_scan'),
  enqueue: (specs: TransferSpec[]) =>
    call<RawJob[]>('queue_transfers', { specs }).then((jobs) => jobs.map(normalizeJob)),
  jobs: (limit = 200, offset = 0, filter = 'all') =>
    call<RawJob[]>('list_transfers', { limit, offset, filter }).then((jobs) =>
      jobs.map(normalizeJob),
    ),
  summary: () => call<{ status: string; count: number }[]>('transfer_summary'),
  control: (id: string, action: string) => call<void>('control_transfer', { id, action }),
  clear: () => call<void>('clear_finished'),
  settings: (concurrency: number) => call<void>('set_concurrency', { concurrency }),
  options: () =>
    call<{
      concurrency: number;
      skipUploadHash: boolean;
      customUploadPartSize: number;
      reportRealSize: boolean;
      useLargeThumbnail: boolean;
      useOldStreamUpload: boolean;
    }>('get_options'),
  foreground: () => call<ForegroundStatus>('get_foreground_status'),
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
interface RawJob extends Omit<TransferJob, 'size' | 'completed' | 'createdAt'> {
  totalBytes: number;
  completedBytes: number;
  createdAt: number;
}
function normalizeJob(job: RawJob): TransferJob {
  return {
    ...job,
    size: job.totalBytes,
    completed: job.completedBytes,
    createdAt: new Date(job.createdAt * 1000).toISOString(),
  };
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
