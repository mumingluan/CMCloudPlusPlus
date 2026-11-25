// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RepositorySettings } from './RepositorySettings';
const api = vi.hoisted(() => ({ caches: vi.fn(), deleteCache: vi.fn(), forgetAll: vi.fn() }));
vi.mock('./repositoryApi', () => ({ repositoryApi: api, repositoryDemo: false }));
vi.mock('./api', () => ({ native: true, errorText: String }));
beforeEach(() => {
  vi.resetAllMocks();
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});
afterEach(cleanup);
it('requires two confirmations before clearing saved repository credentials and suppresses a repeated final click', async () => {
  let resolve!: (count: number) => void;
  api.forgetAll.mockReturnValue(
    new Promise<number>((done) => {
      resolve = done;
    }),
  );
  const forgotten = vi.fn();
  render(<RepositorySettings onForgotten={forgotten} />);
  fireEvent.click(screen.getByRole('button', { name: '删除本机全部对象库凭据存储' }));
  expect(api.forgetAll).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '继续确认' }));
  expect(api.forgetAll).not.toHaveBeenCalled();
  expect(screen.getByRole('heading', { name: '再次确认删除全部凭据' })).toBeTruthy();
  const button = screen.getByRole('button', { name: '确认删除全部凭据' });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(api.forgetAll).toHaveBeenCalledTimes(1);
  expect(forgotten).not.toHaveBeenCalled();
  await act(async () => resolve(2));
  await waitFor(() => expect(forgotten).toHaveBeenCalledOnce());
  expect(screen.queryByRole('dialog')).toBeNull();
});
it('shows cache names, IDs and sizes while preventing deletion of a library in use', async () => {
  api.caches.mockResolvedValue([
    { id: 'a', repositoryId: '1'.repeat(32), name: '归档', bytes: 1048576, busy: true },
    { id: 'b', repositoryId: '2'.repeat(32), name: '历史', bytes: 2097152, busy: false },
  ]);
  api.deleteCache.mockResolvedValue(undefined);
  render(<RepositorySettings />);
  fireEvent.click(screen.getByRole('button', { name: '管理本机对象库清单缓存' }));
  await screen.findByText('归档');
  expect(screen.getByText('1'.repeat(32))).toBeTruthy();
  expect(screen.getByText('2 MB')).toBeTruthy();
  expect(screen.getByRole('button', { name: '删除 归档 的缓存' }).hasAttribute('disabled')).toBe(
    true,
  );
  fireEvent.click(screen.getByRole('button', { name: '删除 历史 的缓存' }));
  await waitFor(() => expect(api.deleteCache).toHaveBeenCalledWith('b'));
  expect(api.forgetAll).not.toHaveBeenCalled();
});
