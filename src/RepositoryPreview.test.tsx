// @vitest-environment jsdom
import { StrictMode } from 'react';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { VideoPreview } from './App';
import type { RepositoryView } from './repositoryApi';
const calls = vi.hoisted(() => ({
  start: vi.fn(),
  ready: vi.fn(),
  release: vi.fn(),
  source: vi.fn(),
  preview: vi.fn(),
  link: vi.fn(),
  open: vi.fn(),
  player: vi.fn(),
}));
vi.mock('./api', async (original) => {
  const api = await original<typeof import('./api')>();
  return {
    ...api,
    api: { ...api.api, preview: calls.preview, link: calls.link, openCompleted: calls.open },
  };
});
vi.mock('./repositoryApi', async (original) => {
  const api = await original<typeof import('./repositoryApi')>();
  return {
    ...api,
    repositoryPreviewUrl: calls.source,
    repositoryApi: {
      ...api.repositoryApi,
      startPreview: calls.start,
      previewReady: calls.ready,
      releasePreview: calls.release,
      openMediaPlayer: calls.player,
    },
  };
});
const repository = {
  handle: 'handle',
  repositoryId: 'a'.repeat(32),
  revision: 'verified-directory',
  encrypted: true,
} as RepositoryView;
const file = {
  id: 'repository:a:movie.mp4',
  path: 'movie.mp4',
  name: 'movie.mp4',
  size: 100,
  isDir: false,
  modifiedAt: '',
};
const job = {
  id: 'preview-job',
  name: 'movie.mp4',
  direction: 'download',
  status: 'queued',
  totalBytes: 100,
  completedBytes: 0,
  createdAt: 0,
  speed: 0,
  localPath: 'R:/Temp/preview/movie.mp4',
};
beforeEach(() => {
  vi.resetAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
  calls.source.mockReturnValue('asset://private/movie.mp4');
  calls.release.mockResolvedValue(undefined);
  calls.open.mockResolvedValue(undefined);
  calls.player.mockResolvedValue(undefined);
});
it('plays an encrypted stream without queuing a preview download and revokes it on close', async () => {
  const url = 'http://127.0.0.1:12345/ticket/movie.mkv';
  calls.start.mockResolvedValue({ url, job: null, streamId: 'stream-ticket' });
  const queued = vi.fn();
  const view = render(
    <StrictMode>
      <VideoPreview file={{ ...file, name: 'movie.mkv' }} repository={repository}
        onQueued={queued} onClose={() => {}} onDownload={() => {}} alert={() => {}} />
    </StrictMode>,
  );
  const player = await screen.findByRole('button', { name: '外部播放器' });
  expect(view.container.querySelector('video')?.getAttribute('src')).toBe(url);
  expect(calls.start).toHaveBeenCalledTimes(1);
  expect(calls.ready).not.toHaveBeenCalled();
  expect(queued).not.toHaveBeenCalled();
  expect((screen.getByRole('button', { name: '复制播放链接' }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.error(view.container.querySelector('video')!);
  expect(screen.getByRole('alert').textContent).toContain('可尝试外部播放器');
  fireEvent.click(player);
  expect(calls.player).toHaveBeenCalledWith('stream-ticket');
  view.unmount();
  await waitFor(() => expect(calls.release).toHaveBeenCalledExactlyOnceWith('stream-ticket'));
});
afterEach(cleanup);
it('uses the media workspace and one queued authenticated download across strict effect replay', async () => {
  calls.start.mockResolvedValue({ url: null, job });
  calls.ready
    .mockResolvedValueOnce({ path: null, status: 'running' })
    .mockResolvedValue({ path: 'R:/Temp/preview/movie.mp4', status: 'completed' });
  const queued = vi.fn();
  const view = render(
    <StrictMode>
      <VideoPreview
        file={file}
        repository={repository}
        onQueued={queued}
        onClose={() => {}}
        onDownload={() => {}}
        alert={() => {}}
      />
    </StrictMode>,
  );
  await screen.findByText('正在下载并解密，可在传输列表查看进度…');
  expect(calls.source).not.toHaveBeenCalled();
  await waitFor(() =>
    expect(view.container.querySelector('video')?.getAttribute('src')).toBe(
      'asset://private/movie.mp4',
    ),
  );
  expect(calls.start).toHaveBeenCalledTimes(1);
  expect(queued).toHaveBeenCalledTimes(1);
  expect(calls.preview).not.toHaveBeenCalled();
  expect(calls.link).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '外部预览' }));
  expect(calls.open).toHaveBeenCalledWith('preview-job');
  view.unmount();
  await waitFor(() => expect(calls.release).toHaveBeenCalledWith('preview-job'));
  expect(calls.release).toHaveBeenCalledTimes(1);
});
it('releases an unfinished preview that resolves after its workspace closes without granting it to the renderer', async () => {
  let resolve!: (value: unknown) => void;
  calls.start.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const queued = vi.fn();
  const view = render(
    <VideoPreview
      file={file}
      repository={repository}
      onQueued={queued}
      onClose={() => {}}
      onDownload={() => {}}
      alert={() => {}}
    />,
  );
  view.unmount();
  resolve({ url: null, job });
  await waitFor(() => expect(calls.release).toHaveBeenCalledWith('preview-job'));
  expect(calls.ready).not.toHaveBeenCalled();
  expect(calls.source).not.toHaveBeenCalled();
  expect(queued).not.toHaveBeenCalled();
});
