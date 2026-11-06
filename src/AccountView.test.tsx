// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AccountView } from './AccountView';
import type { AccountSummary } from './types';

const mocked = vi.hoisted(() => ({
  official: vi.fn(),
  switchSpace: vi.fn(),
  login: vi.fn(),
  accounts: vi.fn(),
  selectAccount: vi.fn(),
  removeAccount: vi.fn(),
}));
vi.mock('./api', () => ({
  api: mocked,
  native: true,
  mobile: false,
  errorText: (e: unknown) => String(e),
}));
const account: AccountSummary = {
  id: 'test-account',
  name: '测试账号',
  account: '138****0000',
  space: 'personal_new',
  rootId: '/',
  cloudId: '',
  userDomainId: '',
  connected: true,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const props = () => ({
  account,
  quota: null,
  onConnected: vi.fn(),
  onLogout: vi.fn(),
  alert: vi.fn(),
});
beforeEach(() => {
  vi.resetAllMocks();
  mocked.accounts.mockResolvedValue([account]);
  mocked.official.mockResolvedValue({ status: '可复用会话' });
});
afterEach(cleanup);

describe('account connection workflow', () => {
  it('keeps the connected space unchanged until confirmation and submits one pending request', async () => {
    const pending = deferred<AccountSummary>();
    mocked.switchSpace.mockReturnValue(pending.promise);
    const options = props();
    render(<AccountView {...options} />);
    fireEvent.change(screen.getByRole('combobox', { name: '目标空间' }), {
      target: { value: 'group' },
    });
    expect(mocked.switchSpace).not.toHaveBeenCalled();
    expect(document.querySelector('.account-space-tag')?.textContent).toBe('个人云');
    fireEvent.change(screen.getByRole('textbox', { name: '群组 ID' }), {
      target: { value: ' group-1 ' },
    });
    const form = screen.getByRole('form', { name: '切换云空间' });
    fireEvent.submit(form);
    fireEvent.submit(form);
    expect(mocked.switchSpace).toHaveBeenCalledTimes(1);
    expect(mocked.switchSpace).toHaveBeenCalledWith('group', 'group-1', '');
    expect(options.onConnected).not.toHaveBeenCalled();
    await act(async () => pending.resolve({ ...account, space: 'group', cloudId: 'group-1' }));
    expect(options.onConnected).toHaveBeenCalledTimes(1);
  });

  it('keeps the current account after a failed switch and allows retry in the same form', async () => {
    mocked.switchSpace
      .mockRejectedValueOnce('群组无访问权限')
      .mockResolvedValueOnce({ ...account, space: 'group' });
    const options = props();
    render(<AccountView {...options} />);
    fireEvent.change(screen.getByRole('combobox', { name: '目标空间' }), {
      target: { value: 'group' },
    });
    fireEvent.change(screen.getByRole('textbox', { name: '群组 ID' }), {
      target: { value: 'group-1' },
    });
    fireEvent.submit(screen.getByRole('form', { name: '切换云空间' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('群组无访问权限'));
    expect(options.onConnected).not.toHaveBeenCalled();
    expect(document.querySelector('.account-space-tag')?.textContent).toBe('个人云');
    fireEvent.submit(screen.getByRole('form', { name: '切换云空间' }));
    await waitFor(() => expect(options.onConnected).toHaveBeenCalledTimes(1));
  });

  it('reveals SMS continuation after risk control and retains password context for retry', async () => {
    mocked.login.mockRejectedValueOnce('SMS_REQUIRED|scene=2').mockResolvedValueOnce(account);
    const options = props();
    render(<AccountView {...options} account={null} />);
    fireEvent.change(screen.getByRole('combobox', { name: '登录方式' }), {
      target: { value: 'password' },
    });
    fireEvent.change(screen.getByLabelText('139 邮箱账号 / 手机号'), {
      target: { value: '13800000000' },
    });
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'test-only-password' } });
    fireEvent.submit(screen.getByRole('form', { name: '登录移动云盘' }));
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toContain('填写收到的验证码'),
    );
    const code = screen.getByLabelText('短信验证码');
    expect(code.closest('details')?.open).toBe(true);
    fireEvent.change(code, { target: { value: '123456' } });
    fireEvent.submit(screen.getByRole('form', { name: '登录移动云盘' }));
    await waitFor(() => expect(options.onConnected).toHaveBeenCalledTimes(1));
    expect(mocked.login.mock.calls[1][0]).toMatchObject({
      username: '13800000000',
      password: 'test-only-password',
      smsCode: '123456',
      authorization: '',
      space: 'personal_new',
    });
  });
});

describe('saved account management', () => {
  it('connects a saved account only once while selection is pending', async () => {
    const other = { ...account, id: 'second', name: '另一个账号', connected: false };
    mocked.accounts.mockResolvedValue([account, other]);
    const options = props();
    const pending = deferred<AccountSummary>();
    mocked.selectAccount.mockReturnValue(pending.promise);
    render(<AccountView {...options} />);
    const connect = await screen.findByRole('button', { name: '连接' });
    fireEvent.click(connect);
    fireEvent.click(connect);
    expect(mocked.selectAccount).toHaveBeenCalledTimes(1);
    expect(mocked.selectAccount).toHaveBeenCalledWith('second');
    expect(options.onConnected).not.toHaveBeenCalled();
    await act(async () => pending.resolve(other));
    expect(options.onConnected).toHaveBeenCalledWith(other);
  });
  it('removes credentials only after explicit confirmation and updates current state', async () => {
    mocked.removeAccount.mockResolvedValue(undefined);
    const onRemovedCurrent = vi.fn();
    render(<AccountView {...props()} onRemovedCurrent={onRemovedCurrent} />);
    fireEvent.click(screen.getByRole('button', { name: '移除 测试账号' }));
    expect(mocked.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '取消' }));
    expect(mocked.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '移除 测试账号' }));
    fireEvent.click(screen.getByRole('button', { name: '确认移除' }));
    await waitFor(() => expect(onRemovedCurrent).toHaveBeenCalledTimes(1));
    expect(mocked.removeAccount).toHaveBeenCalledWith('test-account');
  });
});
