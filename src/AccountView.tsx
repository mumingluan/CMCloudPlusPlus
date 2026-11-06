import { useEffect, useRef, useState } from 'react';
import {
  Cloud,
  HardDrive,
  Info,
  Loader2,
  LogOut,
  UserRound,
  Trash2,
  Check,
} from 'lucide-react';
import { api, errorText, mobile, native } from './api';
import { initialConfig, spaceNames } from './types';
import type { AccountSummary, LoginConfig, Space } from './types';
import { formatBytes } from './utils';

type Method = 'official' | 'password' | 'authorization' | 'cookies' | 'share';
type Props = {
  account: AccountSummary | null;
  quota: { used: number; total: number } | null;
  onConnected: (account: AccountSummary) => void;
  onLogout: () => Promise<void> | void;
  alert: (message: string, error?: boolean) => void;
  onRemovedCurrent?: () => void;
};

export function AccountView({
  account,
  quota,
  onConnected,
  onLogout,
  alert,
  onRemovedCurrent,
}: Props) {
  const [method, setMethod] = useState<Method>(mobile ? 'password' : 'official');
  const [config, setConfig] = useState<LoginConfig>({ ...initialConfig });
  const [space, setSpace] = useState<Space>(account?.space ?? 'personal_new');
  const [cloudId, setCloudId] = useState(account?.cloudId ?? '');
  const [linkId, setLinkId] = useState('');
  const [loginOpen, setLoginOpen] = useState(!account);
  const [busy, setBusy] = useState<'login' | 'space' | 'logout' | 'session' | null>(null);
  const [accounts, setAccounts] = useState<AccountSummary[]>(account ? [account] : []);
  const [removing, setRemoving] = useState<string | null>(null);
  const [sessionError, setSessionError] = useState('');
  const [loginError, setLoginError] = useState('');
  const [spaceError, setSpaceError] = useState('');
  const [smsRequired, setSmsRequired] = useState(false);
  const [discovery, setDiscovery] = useState<Record<string, unknown> | null>(null);
  const submitting = useRef(false);

  useEffect(() => {
    setSpace(account?.space ?? 'personal_new');
    setCloudId(account?.cloudId ?? '');
    setLinkId('');
    setSpaceError('');
    setLoginOpen(!account);
  }, [account?.id, account?.space, account?.cloudId]);

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
        await api.removeAccount(id);
        if (account?.id === id) onRemovedCurrent?.();
        setRemoving(null);
        alert('已移除保存的会话');
      } else {
        onConnected(await api.selectAccount(id));
        setConfig({ ...initialConfig });
        alert('已切换账号；原账号未完成的传输已暂停');
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
  const submitLogin = async () => {
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
              smsCode: method === 'password' ? config.smsCode.trim() : '',
              mailCookies: method === 'password' || method === 'cookies' ? config.mailCookies : '',
              space: method === 'share' ? 'share' : config.space,
              cloudId: method !== 'share' && config.space === 'group' ? config.cloudId.trim() : '',
              linkId: method === 'share' ? config.linkId.trim() : '',
            });
      setConfig({ ...initialConfig });
      setSmsRequired(false);
      setLoginOpen(false);
      onConnected(result);
      alert('云盘已连接');
    } catch (error) {
      const text = errorText(error);
      if (text.startsWith('SMS_REQUIRED')) {
        setSmsRequired(true);
        setLoginError('需要短信验证。填写收到的验证码后，再次登录。');
      } else setLoginError(text);
    } finally {
      submitting.current = false;
      setBusy(null);
    }
  };
  const submitSpace = async () => {
    if (submitting.current || !native || !account) return;
    if (space === 'group' && !cloudId.trim()) {
      setSpaceError('请填写要连接的群组 ID。');
      return;
    }
    if (space === 'share' && !linkId.trim()) {
      setSpaceError('请填写分享链接或分享 ID。');
      return;
    }
    submitting.current = true;
    setBusy('space');
    setSpaceError('');
    try {
      const result = await api.switchSpace(
        space,
        space === 'group' ? cloudId.trim() : '',
        space === 'share' ? linkId.trim() : '',
      );
      onConnected(result);
      alert(`已连接${spaceNames[result.space]}`);
    } catch (error) {
      setSpaceError(errorText(error));
    } finally {
      submitting.current = false;
      setBusy(null);
    }
  };

  const logout = async () => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy('logout');
    try {
      await onLogout();
    } catch (error) {
      setSpaceError(errorText(error));
    } finally {
      submitting.current = false;
      setBusy(null);
    }
  };

  return (
    <section className="workspace account-workspace">
      <div className="page-heading">
        <h1>会话管理</h1>
      </div>
      <div className="account-content">
        <section className="saved-sessions" aria-label="已保存账号">
          <div className="session-heading">
            <h2>已保存账号</h2>
            <span className="muted">{accounts.length} 个</span>
          </div>
          {accounts.map((entry) => (
            <div className="session-row" key={entry.id}>
              <UserRound size={20} />
              <div className="session-label">
                <strong>{entry.name}</strong>
                <small>
                  {entry.account}
                  {entry.id === account?.id && (
                    <span className="account-space-tag">{spaceNames[account.space]}</span>
                  )}
                </small>
              </div>
              {removing === entry.id ? (
                <div className="session-confirm">
                  <span>移除本机凭据？</span>
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
                    确认移除
                  </button>
                </div>
              ) : (
                <div className="session-actions">
                  {entry.id === account?.id ? (
                    <>
                      <span className="session-current">
                        <Check size={14} />
                        当前
                      </span>
                      <button
                        className="icon-button"
                        title="断开连接"
                        aria-label="断开当前会话"
                        disabled={!!busy || !native}
                        onClick={() => void logout()}
                      >
                        <LogOut size={16} />
                      </button>
                    </>
                  ) : (
                    <button
                      className="button secondary"
                      disabled={!!busy || !native}
                      onClick={() => void manageSession(entry.id, false)}
                    >
                      连接
                    </button>
                  )}
                  <button
                    className="icon-button danger"
                    aria-label={`移除 ${entry.name}`}
                    title="移除本机凭据"
                    disabled={!!busy || !native}
                    onClick={() => setRemoving(entry.id)}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              )}
            </div>
          ))}
          {!accounts.length && <p className="muted">登录后会安全保存会话，下次可直接连接。</p>}
          {sessionError && (
            <p className="inline-error" role="alert">
              {sessionError}
            </p>
          )}
        </section>
        {account && (
          <>
            {quota && quota.total > 0 && (
              <div className="account-quota">
                <span>
                  已用 {formatBytes(quota.used)} / {formatBytes(quota.total)}
                </span>
                <progress aria-label="云盘容量使用情况" max={quota.total} value={quota.used} />
              </div>
            )}
            <form
              className="account-space-form"
              aria-label="切换云空间"
              onSubmit={(e) => {
                e.preventDefault();
                void submitSpace();
              }}
            >
              <h2>切换云空间</h2>
              <fieldset disabled={!!busy}>
                <div className="space-form-fields">
                  <label className="form-field">
                    目标空间
                    <select
                      value={space}
                      onChange={(e) => {
                        setSpace(e.target.value as Space);
                        setSpaceError('');
                      }}
                    >
                      {Object.entries(spaceNames).map(([value, label]) => (
                        <option key={value} value={value}>
                          {label}
                        </option>
                      ))}
                    </select>
                  </label>
                  {space === 'group' && (
                    <label className="form-field">
                      群组 ID
                      <input
                        required
                        value={cloudId}
                        onChange={(e) => {
                          setCloudId(e.target.value);
                          setSpaceError('');
                        }}
                        placeholder="输入要访问的群组 ID"
                      />
                    </label>
                  )}
                </div>
                {space === 'share' && (
                  <label className="form-field">
                    分享链接
                    <textarea
                      required
                      value={linkId}
                      onChange={(e) => {
                        setLinkId(e.target.value);
                        setSpaceError('');
                      }}
                      rows={3}
                      placeholder={'分享链接或 link_id，带提取码：link_id#提取码\n多个分享每行一个'}
                    />
                    <small>分享空间只读，部分下载需要有效账号。</small>
                  </label>
                )}
                {spaceError && <InlineError text={spaceError} />}
                <div className="space-form-action">
                  <p>
                    {space === account.space
                      ? space === 'personal_new'
                        ? '当前正在使用个人云。'
                        : '可连接其他群组或分享。'
                      : '连接成功后进入文件列表。'}
                  </p>
                  <button
                    className="button primary"
                    type="submit"
                    disabled={!native || (space === 'personal_new' && account.space === space)}
                  >
                    {busy === 'space' && <Loader2 size={16} className="spin" />}
                    {busy === 'space'
                      ? '正在连接…'
                      : space === 'share'
                        ? '打开分享'
                        : space === 'group'
                          ? '连接群组'
                          : '连接个人云'}
                  </button>
                </div>
              </fieldset>
            </form>
          </>
        )}
        {account && (
          <button
            className="account-login-toggle"
            aria-expanded={loginOpen}
            aria-controls="account-login-form"
            disabled={!!busy}
            onClick={() => {
              setLoginOpen(!loginOpen);
              setLoginError('');
            }}
          >
            <span>添加账号</span>
            <span>{loginOpen ? '收起' : '展开'}</span>
          </button>
        )}
        {loginOpen && (
          <form
            id="account-login-form"
            className="login-panel"
            aria-label="登录移动云盘"
            onSubmit={(e) => {
              e.preventDefault();
              void submitLogin();
            }}
          >
            <h2>{account ? '登录其他账号' : '连接移动云盘'}</h2>
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
                  <option value="share">免登录打开分享</option>
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
                <>
                  <LoginFields
                    method={method}
                    config={config}
                    field={field}
                    smsRequired={smsRequired}
                  />
                  {method !== 'share' && (
                    <label className="form-field">
                      登录后连接
                      <select
                        value={config.space}
                        onChange={(e) => {
                          setConfig((prev) => ({
                            ...prev,
                            space: e.target.value as Space,
                            rootId: '',
                            cloudId: '',
                          }));
                          setLoginError('');
                        }}
                      >
                        <option value="personal_new">个人云</option>
                        <option value="group">群组云</option>
                      </select>
                    </label>
                  )}
                  {method !== 'share' && config.space === 'group' && (
                    <label className="form-field">
                      群组 ID
                      <input
                        required
                        value={config.cloudId}
                        onChange={(e) => field('cloudId', e.target.value)}
                        placeholder="输入群组 ID"
                      />
                    </label>
                  )}
                  <details className="advanced-login">
                    <summary>高级选项</summary>
                    <label className="form-field">
                      账号显示名称
                      <input value={config.name} onChange={(e) => field('name', e.target.value)} />
                    </label>
                    <label className="form-field">
                      根目录 ID
                      <input
                        value={config.rootId}
                        onChange={(e) => field('rootId', e.target.value)}
                        placeholder="留空使用默认目录"
                      />
                    </label>
                    <label className="form-field">
                      User Domain ID
                      <input
                        value={config.userDomainId}
                        onChange={(e) => field('userDomainId', e.target.value)}
                        placeholder="用于查询容量"
                      />
                    </label>
                  </details>
                </>
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
                    : method === 'share'
                      ? '打开分享'
                      : '登录并连接'}
              </button>
            </fieldset>
            <p className="account-footnote">凭据在本机加密保存，密码和短信验证码不会持久化。</p>
            {!native && <p className="preview-note">当前为界面预览，登录请使用桌面或移动应用。</p>}
          </form>
        )}
      </div>
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
  smsRequired,
}: {
  method: Method;
  config: LoginConfig;
  field: (key: keyof LoginConfig, value: string) => void;
  smsRequired: boolean;
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
          <details open={smsRequired || undefined}>
            <summary>短信验证（遇到风控时填写）</summary>
            <label className="form-field">
              短信验证码
              <input
                value={config.smsCode}
                inputMode="numeric"
                autoComplete="one-time-code"
                onChange={(e) => field('smsCode', e.target.value)}
                placeholder="填写收到的验证码"
              />
            </label>
          </details>
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
      {method === 'share' && (
        <label className="form-field">
          分享链接
          <textarea
            required
            spellCheck={false}
            value={config.linkId}
            onChange={(e) => field('linkId', e.target.value)}
            placeholder={'分享链接或 link_id\n带提取码：link_id#提取码\n多个分享每行一个'}
            rows={5}
          />
          <small>分享空间只读，部分下载需要有效账号。</small>
        </label>
      )}
    </>
  );
}
