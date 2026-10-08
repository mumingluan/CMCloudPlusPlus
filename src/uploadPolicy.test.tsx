// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';
import type { TransferJob } from './types';

const mocked = vi.hoisted(() => ({ upload: vi.fn(), download: vi.fn() }));
vi.mock('./api', () => ({
  api: mocked,
  native: false,
  mobile: false,
  android: false,
  errorText: String,
  watchScan: async () => () => {},
  watchTransfers: async () => () => {},
  watchForeground: async () => () => {},
  watchDirectory: async () => () => {},
}));
vi.mock('./preview', () => ({
  scalePreview: () => ({
    account: { id: 'test', space: 'group', rootId: '/', connected: true },
    files: [
      { id: 'file', name: '资料.txt', path: '/file', isDir: false, size: 10, modifiedAt: '' },
    ],
    jobs: [],
  }),
}));
const job: TransferJob = {
  id: 'job',
  name: '资料.txt',
  direction: 'upload',
  status: 'queued',
  size: 10,
  completed: 0,
  speed: 0,
  localPath: '资料.txt',
  createdAt: '',
};
const openTransfers = () => fireEvent.click(screen.getByRole('button', { name: '传输列表' }));
const openFiles = () => fireEvent.click(screen.getByRole('button', { name: '我的文件' }));
const upload = async () => {
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '上传文件' })));
};
beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  history.replaceState(null, '', '/?preview=scale');
  mocked.download.mockResolvedValue([{ ...job, direction: 'download' }]);
});
afterEach(cleanup);

it('keeps a one-time upload policy through cancellation and download, then consumes it on upload', async () => {
  mocked.upload.mockResolvedValueOnce([]).mockResolvedValue([job]);
  render(<App />);
  expect(screen.queryByRole('button', { name: '单次跳过' })).toBeNull();
  openTransfers();
  fireEvent.click(screen.getByRole('button', { name: '单次跳过' }));
  openFiles();
  await upload();
  fireEvent.click(screen.getByRole('checkbox', { name: '选择所有已加载文件' }));
  await act(async () => fireEvent.click(screen.getByRole('button', { name: '下载' })));
  openTransfers();
  expect(screen.getByRole('button', { name: '单次跳过' }).getAttribute('aria-pressed')).toBe(
    'true',
  );
  openFiles();
  await upload();
  openTransfers();
  expect(screen.getByRole('button', { name: '按设置' }).getAttribute('aria-pressed')).toBe('true');
  openFiles();
  await upload();
  expect(mocked.upload.mock.calls.map((args) => args[1])).toEqual([true, true, false]);
});

it('keeps the session policy for subsequent upload batches across page changes', async () => {
  mocked.upload.mockResolvedValue([job]);
  render(<App />);
  openTransfers();
  fireEvent.click(screen.getByRole('button', { name: '本会话跳过' }));
  openFiles();
  await upload();
  openTransfers();
  expect(screen.getByRole('button', { name: '本会话跳过' }).getAttribute('aria-pressed')).toBe(
    'true',
  );
  openFiles();
  await upload();
  expect(mocked.upload.mock.calls.map((args) => args[1])).toEqual([true, true]);
});
