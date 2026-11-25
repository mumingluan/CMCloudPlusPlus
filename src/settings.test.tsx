// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { SettingsView } from './App';
import type { AccountSummary } from './types';

const mocked = vi.hoisted(() => ({ options: vi.fn(), settings: vi.fn(), updateOptions: vi.fn() }));
vi.mock('./api', () => ({
  api: mocked,
  native: true,
  mobile: false,
  android: false,
  errorText: String,
}));
const account: AccountSummary = {
  id: 'saved',
  name: 'account',
  phone: '19900001111',
  account: '199****1111',
  space: 'personal_new',
  rootId: '/',
  cloudId: '',
  userDomainId: '',
  connected: true,
};
const options = {
  concurrency: 4,
  customUploadPartSize: 0,
  useLargeThumbnail: false,
};
const props = () => ({
  account,
  run: async (action: () => Promise<unknown>) => {
    await action();
  },
  onAccount: vi.fn(),
  systemDark: false,
  oled: false,
  onOled: vi.fn(),
  theme: 'light' as const,
  onTheme: vi.fn(),
  hideVendorFolders: true,
  onHideVendorFolders: vi.fn(),
  sessionsOpen: false,
  onSessionsOpen: vi.fn(),
  onSettingsBack: vi.fn(),
  children: <section aria-label="账号管理">session controls</section>,
});
beforeEach(() => {
  vi.resetAllMocks();
  mocked.options.mockResolvedValue(options);
  mocked.updateOptions.mockResolvedValue(account);
});
afterEach(cleanup);

it('submits the staged settings once and keeps account updates separate from session navigation', async () => {
  let finish!: () => void;
  mocked.settings.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  const settings = props();
  render(<SettingsView {...settings} />);
  const save = screen.getByRole('button', { name: '保存设置' }) as HTMLButtonElement;
  await waitFor(() =>
    expect(
      (screen.getByRole('combobox', { name: '同时传输任务数' }) as HTMLSelectElement).disabled,
    ).toBe(false),
  );
  expect(save.disabled).toBe(true);
  fireEvent.change(screen.getByRole('combobox', { name: '同时传输任务数' }), {
    target: { value: '6' },
  });
  fireEvent.change(screen.getByRole('combobox', { name: '上传分片大小' }), {
    target: { value: String(32 * 1024 * 1024) },
  });
  fireEvent.click(save);
  fireEvent.click(save);
  expect(mocked.settings).toHaveBeenCalledTimes(1);
  expect(mocked.settings).toHaveBeenCalledWith(6);
  expect(save.disabled).toBe(true);
  await act(async () => finish());
  expect(mocked.updateOptions).toHaveBeenCalledWith({
    customUploadPartSize: 32 * 1024 * 1024,
    useLargeThumbnail: false,
  });
  expect(settings.onAccount).toHaveBeenCalledWith(account);
  expect(settings.onSessionsOpen).not.toHaveBeenCalled();
  expect(screen.queryByRole('region', { name: '账号管理' })).toBeNull();
  expect(save.disabled).toBe(true);
  expect(document.querySelectorAll('.setting-row.is-modified')).toHaveLength(0);
});

it('allows retry after reading settings fails instead of leaving saving permanently disabled', async () => {
  mocked.options.mockRejectedValueOnce('temporary failure').mockResolvedValue(options);
  render(<SettingsView {...props()} />);
  await screen.findByRole('alert');
  expect((screen.getByRole('button', { name: '保存设置' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  await waitFor(() =>
    expect(
      (screen.getByRole('combobox', { name: '同时传输任务数' }) as HTMLSelectElement).disabled,
    ).toBe(false),
  );
  expect((screen.getByRole('button', { name: '保存设置' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  expect(screen.queryByRole('alert')).toBeNull();
});

it('highlights only pending rows and becomes clean when changes are reverted', async () => {
  render(<SettingsView {...props()} />);
  const select = screen.getByRole('combobox', { name: '同时传输任务数' }) as HTMLSelectElement;
  await waitFor(() => expect(select.disabled).toBe(false));
  const save = screen.getByRole('button', { name: '保存设置' }) as HTMLButtonElement;
  expect(save.disabled).toBe(true);
  fireEvent.change(select, { target: { value: '6' } });
  expect(save.disabled).toBe(false);
  expect(select.closest('.setting-row')?.classList.contains('is-modified')).toBe(true);
  expect(document.querySelectorAll('.setting-row.is-modified')).toHaveLength(1);
  fireEvent.change(select, { target: { value: '4' } });
  expect(save.disabled).toBe(true);
  expect(document.querySelectorAll('.setting-row.is-modified')).toHaveLength(0);
  expect(mocked.settings).not.toHaveBeenCalled();
});

it('retains pending edits after a failed save and clears only successfully saved values', async () => {
  mocked.settings.mockResolvedValue(undefined);
  mocked.updateOptions.mockRejectedValueOnce('write failed').mockResolvedValue(account);
  const settings = props();
  settings.run = async (action) => {
    try {
      await action();
    } catch {}
  };
  render(<SettingsView {...settings} />);
  const select = screen.getByRole('combobox', { name: '同时传输任务数' }) as HTMLSelectElement;
  await waitFor(() => expect(select.disabled).toBe(false));
  fireEvent.change(select, { target: { value: '6' } });
  const real = screen.getByRole('switch', { name: '使用大尺寸缩略图' });
  fireEvent.click(real);
  const save = screen.getByRole('button', { name: '保存设置' }) as HTMLButtonElement;
  fireEvent.click(save);
  await waitFor(() => expect(mocked.updateOptions).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(save.disabled).toBe(false));
  expect(select.closest('.setting-row')?.classList.contains('is-modified')).toBe(false);
  expect(real.closest('.setting-row')?.classList.contains('is-modified')).toBe(true);
  fireEvent.click(save);
  await waitFor(() => expect(save.disabled).toBe(true));
  expect(mocked.settings).toHaveBeenCalledTimes(1);
  expect(mocked.updateOptions).toHaveBeenCalledTimes(2);
  expect(document.querySelectorAll('.setting-row.is-modified')).toHaveLength(0);
});
