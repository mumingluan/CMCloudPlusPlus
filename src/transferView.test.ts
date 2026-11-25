import { expect, it } from 'vitest';
import { matchesTransfer, progressSegments, overviewFromJobs } from './transferView';
import { isVendorFolder, vendorFolders } from './utils';
import type { TransferJob, CloudFile } from './types';

it('separates transfer direction, completion, errors and cancelled jobs', () => {
  expect(matchesTransfer({ direction: 'upload', status: 'paused' }, 'upload_active')).toBe(true);
  expect(matchesTransfer({ direction: 'download', status: 'failed' }, 'upload_active', true)).toBe(
    false,
  );
  expect(
    matchesTransfer({ direction: 'download', status: 'failed' }, 'download_active', true),
  ).toBe(true);
  expect(matchesTransfer({ direction: 'download', status: 'running' }, 'active', true)).toBe(false);
  expect(matchesTransfer({ direction: 'download', status: 'cancelled' }, 'completed')).toBe(false);
  expect(matchesTransfer({ direction: 'upload', status: 'completed' }, 'upload_completed')).toBe(
    true,
  );
});

it('assigns completed and pending bytes to the correct upload/download colors', () => {
  const overview = {
    upload: { totalJobs: 2, completedJobs: 1, totalBytes: 200, completedBytes: 100 },
    download: { totalJobs: 1, completedJobs: 0, totalBytes: 100, completedBytes: 25 },
    uploadSpeed: 10,
    downloadSpeed: 20,
  };
  const combined = progressSegments(overview, 'active');
  expect(combined.completed).toBe(125);
  expect(combined.total).toBe(300);
  expect(combined.segments).toEqual([
    { kind: 'upload-done', bytes: 100 },
    { kind: 'download-done', bytes: 25 },
    { kind: 'upload-pending', bytes: 100 },
    { kind: 'download-pending', bytes: 75 },
  ]);
  expect(progressSegments(overview, 'upload_active').percent).toBe(50);
  expect(progressSegments(overview, 'download_active').percent).toBe(25);
});

it('does not count hash scan throughput as uploaded data or network speed', () => {
  const job: TransferJob = {
    id: 'x',
    name: 'x.bin',
    direction: 'upload',
    status: 'hashing',
    size: 100,
    completed: 70,
    speed: 1000,
    localPath: '',
    createdAt: '',
  };
  const overview = overviewFromJobs([job]);
  expect(overview.upload.completedBytes).toBe(0);
  expect(overview.uploadSpeed).toBe(0);
  expect(progressSegments(overviewFromJobs([]), 'active').percent).toBe(0);
});

it('hides only the four vendor directories at the cloud root', () => {
  const file: CloudFile = { id: 'x', name: '', path: '/x', isDir: true, size: 0, modifiedAt: '' };
  for (const name of vendorFolders) {
    expect(isVendorFolder({ ...file, name }, true)).toBe(true);
    expect(isVendorFolder({ ...file, name }, false)).toBe(false);
    expect(isVendorFolder({ ...file, name, isDir: false }, true)).toBe(false);
  }
  expect(isVendorFolder({ ...file, name: '我的文件' }, true)).toBe(false);
});
