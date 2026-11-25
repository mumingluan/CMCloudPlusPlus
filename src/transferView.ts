import type { TransferJob } from './types';

export type TransferFilter =
  | 'upload_active'
  | 'upload_completed'
  | 'download_active'
  | 'download_completed'
  | 'active'
  | 'completed';
export const transferTabs: [TransferFilter, string][][] = [
  [
    ['upload_active', '上传中'],
    ['upload_completed', '已上传'],
  ],
  [
    ['download_active', '下载中'],
    ['download_completed', '已下载'],
  ],
  [
    ['active', '全部进行中'],
    ['completed', '全部已完成'],
  ],
];
export const isOngoing = (filter: TransferFilter) =>
  filter === 'active' || filter.endsWith('_active');
export function matchesTransfer(
  job: { direction?: string; status: string },
  filter: TransferFilter,
  errorsOnly = false,
) {
  if (filter.startsWith('upload_') && job.direction !== 'upload') return false;
  if (filter.startsWith('download_') && job.direction !== 'download') return false;
  if (!isOngoing(filter)) return job.status === 'completed';
  return errorsOnly ? job.status === 'failed' : !['completed', 'cancelled'].includes(job.status);
}
export interface DirectionOverview {
  totalJobs: number;
  completedJobs: number;
  totalBytes: number;
  completedBytes: number;
}
export interface TransferOverview {
  upload: DirectionOverview;
  download: DirectionOverview;
  uploadSpeed: number;
  downloadSpeed: number;
}
export function overviewFromJobs(jobs: TransferJob[]): TransferOverview {
  const result: TransferOverview = {
    upload: { totalJobs: 0, completedJobs: 0, totalBytes: 0, completedBytes: 0 },
    download: { totalJobs: 0, completedJobs: 0, totalBytes: 0, completedBytes: 0 },
    uploadSpeed: 0,
    downloadSpeed: 0,
  };
  for (const job of jobs) {
    if (job.status === 'cancelled') continue;
    const direction = result[job.direction];
    direction.totalJobs++;
    direction.totalBytes += job.size;
    direction.completedBytes +=
      job.status === 'completed'
        ? job.size
        : ['hashing','encrypting','decrypting','publishing'].includes(job.status)
          ? 0
          : Math.min(job.completed, job.size);
    if (job.status === 'completed') direction.completedJobs++;
    if (['running', 'uploading', 'downloading'].includes(job.status))
      result[job.direction === 'upload' ? 'uploadSpeed' : 'downloadSpeed'] += job.speed;
  }
  return result;
}

export function progressSegments(overview: TransferOverview, filter: TransferFilter) {
  const upload =
    filter === 'download_active' ? { totalBytes: 0, completedBytes: 0 } : overview.upload;
  const download =
    filter === 'upload_active' ? { totalBytes: 0, completedBytes: 0 } : overview.download;
  const total = upload.totalBytes + download.totalBytes;
  const completed = Math.min(total, upload.completedBytes + download.completedBytes);
  return {
    total,
    completed,
    percent: total > 0 ? (completed / total) * 100 : 0,
    segments: [
      { kind: 'upload-done', bytes: upload.completedBytes },
      { kind: 'download-done', bytes: download.completedBytes },
      { kind: 'upload-pending', bytes: Math.max(0, upload.totalBytes - upload.completedBytes) },
      {
        kind: 'download-pending',
        bytes: Math.max(0, download.totalBytes - download.completedBytes),
      },
    ].filter((segment) => segment.bytes > 0),
  };
}
