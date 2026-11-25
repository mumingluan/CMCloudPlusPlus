import { useEffect, useId, useRef, useState } from 'react';
import { Cloud, HardDrive, Info, Loader2, Plus, UserRound, Trash2, Check, X } from 'lucide-react';
import { api, errorText, mobile, native } from './api';
import { initialConfig } from './types';
import { RepositoryDialog } from './RepositoryDialog';
import type { AccountSummary, LoginConfig } from './types';
import { formatBytes } from './utils';

type Method = 'official' | 'password' | 'authorization' | 'cookies';
type Props = {
  embedded?: boolean;
  account: AccountSummary | null;
  quota: { used: number; total: number } | null;
  onConnected: (account: AccountSummary) => void;
  alert: (message: string, error?: boolean) => void;
};

export function AccountView({
  account,
  quota,
  onConnected,
  alert,
  embedded = false,
}: Props) {
  const [method, setMethod] = useState<Method>(mobile ? 'password' : 'official');
  const [config, setConfig] = useState<LoginConfig>({ ...initialConfig });
  const [loginOpen, setLoginOpen] = useState(false);
  const titleId = useId();
  const smsTitleId = useId();
  const [busy, setBusy] = useState<'login' | 'session' | null>(null);
  const [accounts, setAccounts] = useState<AccountSummary[]>(account ? [account] : []);
  const [removing, setRemoving] = useState<string | null>(null);
  const [sessionError, setSessionError] = useState('');
  const [loginError, setLoginError] = useState('');
  const [smsRequired, setSmsRequired] = useState(false);
  const [discovery, setDiscovery] = useState<Record<string, unknown> | null>(null);
  const submitting = useRef(false);
  const addAccountButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!native) {
      setAccounts(account ? [account] : []);
      return;
    }
    let disposed = false;
    void api
      .accounts()
      .then((items) => {
        if (!disposed) setAccounts(items);
      })
      .catch((error) => {
        if (!disposed) setSessionError(errorText(error));
      });
    return () => {
      disposed = true;
    };
  }, [account]);

  const manageSession = async (id: string, remove: boolean) => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy('session');
    setSessionError('');
    try {
      if (remove) {
        const active = await api.removeAccount(id);
        if (active) onConnected(active);
        setRemoving(null);
        alert('已删除账号');
      } else {
        onConnected(await api.selectAccount(id));
        setConfig({ ...initialConfig });
        alert('已切换账号');
      }
      setAccounts(await api.accounts());
    } catch (error) {
      setSessionError(errorText(error));
    } finally {
      submitting.current = false;
      setBusy(null);
    }
  };

  useEffect(() => {
    if (!native || mobile || method !== 'official' || !loginOpen) return;
    let cancelled = false;
    void api
      .official()
      .then((value) => {
        if (!cancelled) setDiscovery(value);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [method, loginOpen]);

  const field = (key: keyof LoginConfig, value: string) => {
    setConfig((previous) => ({ ...previous, [key]: value }));
    setLoginError('');
  };
  const submitLogin = async (smsCode = '') => {
    if (submitting.current || !native) return;
    submitting.current = true;
    setBusy('login');
    setLoginError('');
    try {
      const result =
        method === 'official'
          ? await api.importOfficial()
          : await api.login({
              ...config,
              authorization: method === 'authorization' ? config.authorization.trim() : '',
              username: method === 'password' ? config.username.trim() : '',
              password: method === 'password' ? config.password : '',
              smsCode: method === 'password' ? smsCode : '',
              mailCookies: method === 'password' || method === 'cookies' ? config.mailCookies : '',
            });
      setConfig({ ...initialConfig });
      setSmsRequired(false);
      setLoginOpen(false);
      onConnected(result);
      alert('云盘已连接');
    } catch (error) {
      const text = errorText(error);
      if (method === 'password' && text.startsWith('SMS_REQUIRED')) {
        setSmsRequired(true);
        field('smsCode', '');
      } else setLoginError(text);
    } finally {
      submitting.current = false;
      setBusy(null);
    }
  };
  const closeLogin = () => {
    if (submitting.current) return;
    setLoginOpen(false);
    setConfig({ ...initialConfig });
    setSmsRequired(false);
    setLoginError('');
  };
  const cancelVerification = () => {
    if (submitting.current) return;
    setSmsRequired(false);
    field('smsCode', '');
  };

  return (
    <section
      className={embedded ? 'account-section' : 'workspace account-workspace'}
      aria-label="账号管理"
    >
      <div className="account-content">
        <div className="session-heading">
          <h2 className="session-section-title">账号管理</h2>
          <span className="muted">{accounts.length} 个</span>
        </div>
        <section className="saved-sessions" aria-label="账号列表">
          {accounts.map((entry) => (
            <div className="session-row" key={entry.id}>
              <UserRound size={20} />
              <div className="session-label">
                <strong>{entry.phone || entry.account}</strong>
                {entry.id === account?.id && quota && quota.total > 0 && (
                  <small>已用 {formatBytes(quota.used)} / {formatBytes(quota.total)}</small>
                )}
              </div>
              {removing === entry.id ? (
                <div className="session-confirm">
                  <span>删除此账号？</span>
                  <button
                    className="button quiet"
                    disabled={!!busy}
                    onClick={() => setRemoving(null)}
                  >
                    取消
                  </button>
                  <button
                    className="button quiet danger"
                    disabled={!!busy}
                    onClick={() => void manageSession(entry.id, true)}
                  >
                    确认删除
                  </button>
                </div>
              ) : (
                <div className="session-actions">
                  {entry.id === account?.id ? (
                    <span className="session-current">
                      <Check size={14} />
                      当前
                    </span>
                  ) : (
                    <button
                      className="button secondary"
                      disabled={!!busy || !native}
                      onClick={() => void manageSession(entry.id, false)}
                    >
                      切换
                    </button>
                  )}
                  <button
                    className="icon-button danger"
                    aria-label={`删除 ${entry.phone || entry.account}`}
                    title={accounts.length === 1 ? "请先添加另一个账号" : "删除账号"}
                    disabled={!!busy || !native || accounts.length < 2}
                    onClick={() => setRemoving(entry.id)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              )}
            </div>
          ))}
          {!accounts.length && <p className="muted">尚未添加账号</p>}
          {sessionError && (
            <p className="inline-error" role="alert">
              {sessionError}
            </p>
          )}
        </section>
        <button ref={addAccountButton} className="button secondary" aria-haspopup="dialog" disabled={!!busy}
          onClick={() => setLoginOpen(true)}>
          <Plus size={17} />
          添加账号
        </button>
      </div>
      {loginOpen && !smsRequired && (
        <RepositoryDialog titleId={titleId} busy={!!busy} onDismiss={closeLogin}
          returnFocus={addAccountButton.current}>
          <form
            id="account-login-form"
            className="login-panel account-login-modal"
            aria-label="登录移动云盘"
            onSubmit={(e) => {
              e.preventDefault();
              void submitLogin();
            }}
          >
            <header className="dialog-heading">
              <h2 id={titleId}>添加账号</h2>
              <button className="icon-button" type="button" aria-label="关闭" disabled={!!busy}
                onClick={closeLogin}><X size={20} /></button>
            </header>
            <fieldset disabled={!!busy}>
              <label className="form-field">
                登录方式
                <select
                  value={method}
                  onChange={(e) => {
                    setMethod(e.target.value as Method);
                    setLoginError('');
                    setSmsRequired(false);
                  }}
                >
                  {!mobile && <option value="official">复用官方 Windows 客户端</option>}
                  <option value="password">139 邮箱账号与密码</option>
                  <option value="authorization">Authorization 凭据</option>
                  <option value="cookies">139 邮箱 Cookie</option>
                </select>
              </label>
              {method === 'official' ? (
                <div className="official-import">
                  <HardDrive size={28} strokeWidth={1.5} />
                  <div>
                    <h3>复用本机已登录的账号</h3>
                    <p>读取官方客户端会话，验证后连接个人云。</p>
                    <span className="official-status">
                      {discovery
                        ? String(discovery.status ?? discovery.message ?? '已完成客户端检测')
                        : '在 Windows 桌面应用中使用'}
                    </span>
                  </div>
                </div>
              ) : (
                <LoginFields method={method} config={config} field={field} />
              )}
              {loginError && <InlineError text={loginError} />}
              <button className="button primary login-submit" type="submit" disabled={!native}>
                {busy === 'login' ? (
                  <Loader2 size={17} className="spin" />
                ) : method === 'official' ? (
                  <HardDrive size={17} />
                ) : (
                  <Cloud size={17} />
                )}
                {busy === 'login'
                  ? '正在连接…'
                  : method === 'official'
                    ? '检测并复用凭据'
                    : '登录并连接'}
              </button>
            </fieldset>
            {!native && <p className="preview-note">当前为界面预览，登录请使用桌面或移动应用。</p>}
          </form>
        </RepositoryDialog>
      )}
      {loginOpen && smsRequired && (
        <RepositoryDialog compact titleId={smsTitleId} busy={!!busy} onDismiss={cancelVerification}
          returnFocus={addAccountButton.current}>
          <form className="login-panel account-login-modal" aria-label="短信验证"
            onSubmit={(event) => {
              event.preventDefault();
              const code = config.smsCode.trim();
              if (code) void submitLogin(code);
            }}>
            <header className="dialog-heading">
              <h2 id={smsTitleId}>短信验证</h2>
              <button className="icon-button" type="button" aria-label="关闭" disabled={!!busy}
                onClick={cancelVerification}><X size={20} /></button>
            </header>
            <fieldset disabled={!!busy}>
              <label className="form-field">
                短信验证码
                <input required data-dialog-autofocus inputMode="numeric" autoComplete="one-time-code"
                  value={config.smsCode} onChange={(event) => field('smsCode', event.target.value)}
                  placeholder="填写收到的验证码" />
              </label>
              {loginError && <InlineError text={loginError} />}
              <button className="button primary login-submit" type="submit" disabled={!config.smsCode.trim()}>
                {busy === 'login' && <Loader2 size={17} className="spin" />}
                {busy === 'login' ? '正在验证…' : '验证并登录'}
              </button>
            </fieldset>
          </form>
        </RepositoryDialog>
      )}
    </section>
  );
}

function InlineError({ text }: { text: string }) {
  return (
    <div className="inline-error" role="alert">
      <Info size={17} />
      <span>{text}</span>
    </div>
  );
}

function LoginFields({
  method,
  config,
  field,
}: {
  method: Method;
  config: LoginConfig;
  field: (key: keyof LoginConfig, value: string) => void;
}) {
  return (
    <>
      {' '}
      {method === 'password' && (
        <>
          <label className="form-field">
            139 邮箱账号 / 手机号
            <input
              required
              autoComplete="username"
              inputMode="tel"
              value={config.username}
              onChange={(e) => field('username', e.target.value)}
              placeholder="输入账号"
            />
          </label>
          <label className="form-field">
            密码
            <input
              required
              type="password"
              autoComplete="current-password"
              value={config.password}
              onChange={(e) => field('password', e.target.value)}
              placeholder="输入密码"
            />
          </label>
          <details>
            <summary>复用邮箱设备 Cookie（可选）</summary>
            <textarea
              value={config.mailCookies}
              onChange={(e) => field('mailCookies', e.target.value)}
              aria-label="邮箱设备 Cookie"
              rows={3}
            />
          </details>
        </>
      )}
      {method === 'authorization' && (
        <label className="form-field">
          Authorization
          <textarea
            required
            autoComplete="off"
            spellCheck={false}
            value={config.authorization}
            onChange={(e) => field('authorization', e.target.value)}
            placeholder="粘贴 Authorization，支持 Basic 前缀"
            rows={4}
          />
          <small>来自移动云盘的登录凭据，请勿提供给他人。</small>
        </label>
      )}
      {method === 'cookies' && (
        <label className="form-field">
          139 邮箱 Cookie
          <textarea
            required
            autoComplete="off"
            spellCheck={false}
            value={config.mailCookies}
            onChange={(e) => field('mailCookies', e.target.value)}
            placeholder="包含 Os_SSo_Sid 和 RMKEY 的邮箱 Cookie"
            rows={4}
          />
          <small>Cookie 登录保留现有设备上下文。</small>
        </label>
      )}
    </>
  );
}
