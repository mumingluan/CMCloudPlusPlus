// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AccountView } from './AccountView';
import type { AccountSummary } from './types';

const mocked = vi.hoisted(() => ({
  official: vi.fn(),
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
  alert: vi.fn(),
});
beforeEach(() => {
  vi.resetAllMocks();
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
  mocked.accounts.mockResolvedValue([account]);
  mocked.official.mockResolvedValue({ status: '可复用会话' });
});
afterEach(cleanup);

describe('account connection workflow', () => {
  it('shows quota on the active account and protects the last account', async () => {
    render(<AccountView {...props()} quota={{ used: 1024, total: 4096 }} />);
    expect(screen.getByRole('heading', { name: '账号管理' })).toBeTruthy();
    expect(screen.getByText('1 个')).toBeTruthy();
    expect(screen.getByText('已用 1 KB / 4 KB')).toBeTruthy();
    expect(screen.queryByText('切换云空间')).toBeNull();
    expect(screen.queryByRole('dialog')).toBeNull();
    const remove = screen.getByRole('button', { name: '删除 138****0000' }) as HTMLButtonElement;
    expect(remove.disabled).toBe(true);
    fireEvent.click(remove);
    expect(mocked.removeAccount).not.toHaveBeenCalled();
  });

  it('reveals SMS continuation after risk control and retains password context for retry', async () => {
    mocked.login.mockRejectedValueOnce('SMS_REQUIRED|scene=2')
      .mockRejectedValueOnce('验证码错误').mockResolvedValueOnce(account);
    const options = props();
    render(<AccountView {...options} account={null} />);
    fireEvent.click(screen.getByRole('button', { name: '添加账号' }));
    expect(screen.getByRole('dialog', { name: '添加账号' })).toBeTruthy();
    fireEvent.change(screen.getByRole('combobox', { name: '登录方式' }), {
      target: { value: 'password' },
    });
    fireEvent.change(screen.getByLabelText('139 邮箱账号 / 手机号'), {
      target: { value: '13800000000' },
    });
    fireEvent.change(screen.getByLabelText('密码'), { target: { value: 'test-only-password' } });
    expect(screen.queryByLabelText('短信验证码')).toBeNull();
    expect(screen.queryByText('高级选项')).toBeNull();
    fireEvent.submit(screen.getByRole('form', { name: '登录移动云盘' }));
    await screen.findByRole('dialog', { name: '短信验证' });
    expect(screen.queryByRole('dialog', { name: '添加账号' })).toBeNull();
    expect(mocked.login.mock.calls[0][0].smsCode).toBe('');
    const code = screen.getByLabelText('短信验证码');
    expect(document.activeElement).toBe(code);
    fireEvent.change(code, { target: { value: '000000' } });
    const verification = screen.getByRole('form', { name: '短信验证' });
    fireEvent.submit(verification);
    expect((await screen.findByRole('alert')).textContent).toContain('验证码错误');
    fireEvent.change(code, { target: { value: '123456' } });
    fireEvent.submit(verification);
    await waitFor(() => expect(options.onConnected).toHaveBeenCalledTimes(1));
    expect(mocked.login.mock.calls[2][0]).toMatchObject({
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
    const connect = await screen.findByRole('button', { name: '切换' });
    fireEvent.click(connect);
    fireEvent.click(connect);
    expect(mocked.selectAccount).toHaveBeenCalledTimes(1);
    expect(mocked.selectAccount).toHaveBeenCalledWith('second');
    expect(options.onConnected).not.toHaveBeenCalled();
    await act(async () => pending.resolve(other));
    expect(options.onConnected).toHaveBeenCalledWith(other);
  });
  it('deletes the active account after confirmation and uses the replacement', async () => {
    const other = { ...account, id: 'second', account: '139****0000' };
    mocked.accounts.mockResolvedValue([account, other]);
    mocked.removeAccount.mockResolvedValue(other);
    const options = props();
    render(<AccountView {...options} />);
    await screen.findByText('2 个');
    fireEvent.click(screen.getByRole('button', { name: '删除 138****0000' }));
    expect(mocked.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '取消' }));
    expect(mocked.removeAccount).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '删除 138****0000' }));
    fireEvent.click(screen.getByRole('button', { name: '确认删除' }));
    await waitFor(() => expect(options.onConnected).toHaveBeenCalledWith(other));
    expect(mocked.removeAccount).toHaveBeenCalledWith('test-account');
  });
});
