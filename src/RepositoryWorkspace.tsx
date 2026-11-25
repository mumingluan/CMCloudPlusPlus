import { useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import type { Ref } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  Box,
  KeyRound,
  Loader2,
  LockKeyhole,
  RefreshCw,
  Upload,
  X,
  WandSparkles,
} from 'lucide-react';
import { repositoryApi, repositoryDemo } from './repositoryApi';
import type {
  RepositoryProbe,
  RepositoryView,
  RepositoryUnlock,
  RepositoryUploadPlan,
} from './repositoryApi';
import type { CloudFile, TransferJob } from './types';
import { errorText } from './api';
import { formatBytes, validName } from './utils';
import './repository.css';
import { RepositoryDialog } from './RepositoryDialog';

export function AxuanStorMark() {
  return (
    <svg className="axuanstor-mark" role="img" aria-label="AxuanStor 对象库" viewBox="0 0 24 24">
      <rect x="1" y="1" width="22" height="22" rx="6" fill="currentColor" />
      <path
        d="M12 4.2 19 8.2v7.6l-7 4-7-4V8.2Z"
        fill="#fff"
        fillOpacity=".16"
        stroke="#fff"
        strokeWidth="1.25"
        strokeLinejoin="round"
      />
      <path
        d="M12 4.2V12m0 0 7-3.8M12 12 5 8.2"
        stroke="#fff"
        strokeOpacity=".5"
        strokeWidth="1.1"
        strokeLinecap="round"
      />
      <path
        d="m8.6 16.2 3.4-6.8 3.4 6.8m-5.3-2.2h3.8"
        fill="none"
        stroke="#fff"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
const actionText = (action: string) =>
  (({
    skip: '相同路径，跳过',
    reuse: '复用已有内容',
    'batch-reuse': '复用本批文件',
    upload: '上传新内容',
  })[action.replace(/^replace-/, '')] ?? action) +
  (action.startsWith('replace-') ? ' · 替换路径' : '');

function UnlockFields({
  creating,
  changing = false,
  initialMethod = 'password',
  name,
  onNameChange,
  titleId,
  managementName,
  onSubmit,
  busy,
  error,
  onCancel,
  saved,
  onPickerPending,
}: {
  creating?: boolean;
  changing?: boolean;
  initialMethod?: 'password' | 'key';
  name?: string;
  onNameChange?: (name: string) => void;
  titleId?: string;
  managementName?: string;
  onSubmit: (
    u: RepositoryUnlock,
    remember: boolean,
    useSaved?: boolean,
    encrypted?: boolean,
  ) => void;
  busy: boolean;
  error: string;
  onCancel: () => void;
  saved: boolean;
  onPickerPending?: (pending: boolean) => void;
}) {
  const [encrypted, setEncrypted] = useState(!creating);
  const [method, setMethod] = useState<'password' | 'key'>(initialMethod);
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [key, setKey] = useState('');
  const [keyFile, setKeyFile] = useState<{ selectionId: string; name: string } | null>(null);
  const [remember, setRemember] = useState(false);
  const [importError, setImportError] = useState('');
  const [picking, setPicking] = useState(false);
  const pickerBusy = (pending: boolean) => {
    setPicking(pending);
    onPickerPending?.(pending);
  };
  return (
    <form
      className="repository-unlock"
      aria-busy={busy || picking}
      onSubmit={(e) => {
        e.preventDefault();
        if (busy || picking) return;
        if (
          (creating || changing) &&
          encrypted &&
          method === 'password' &&
          password !== confirmation
        ) {
          setImportError('两次输入的密码不一致，请重新确认。');
          return;
        }
        if (onNameChange && !validName(name ?? '')) {
          setImportError('请输入有效的对象库名称。');
          return;
        }
        setImportError('');
        onSubmit(
          encrypted
            ? method === 'password'
              ? { password }
              : { key, keyFile: keyFile?.selectionId }
            : {},
          remember,
          false,
          encrypted,
        );
        setPassword('');
        setConfirmation('');
        setKey('');
        setKeyFile(null);
      }}
    >
      <div className="repository-unlock-icon">
        <LockKeyhole size={28} />
      </div>
      <h2 id={titleId}>
        {managementName
          ? '管理对象库'
          : changing
            ? method === 'password'
              ? '修改对象库密码'
              : '更换对象库密钥'
            : creating
              ? '新建对象库'
              : '解锁对象库'}
      </h2>
      <p>
        {managementName
          ? `解锁 ${managementName} 后即可管理对象库。`
          : changing
            ? '更新所选解锁方式，文件内容保持原样。其他已设置的解锁方式仍可使用。'
            : creating
              ? onNameChange
                ? '在当前云盘目录中创建，按内容校验和管理文件。'
                : '在这个空文件夹中建立对象库。'
              : '在本机解锁后浏览目录和文件。'}
      </p>
      {onNameChange && (
        <label>
          对象库名称
          <input
            autoFocus
            value={name}
            onChange={(e) => onNameChange(e.target.value)}
            required
            maxLength={255}
            disabled={busy || picking}
          />
        </label>
      )}
      {repositoryDemo && <p className="repository-demo">界面演示 · 密码为 demo</p>}
      {creating && (
        <label>
          对象库类型
          <select
            disabled={busy || picking}
            value={encrypted ? 'encrypted' : 'plain'}
            onChange={(e) => setEncrypted(e.target.value === 'encrypted')}
          >
            <option value="plain">普通对象库</option>
            <option value="encrypted">加密对象库</option>
          </select>
        </label>
      )}
      {encrypted && (
        <>
          <div className="repository-method" role="group" aria-label="解锁方式">
            <button
              type="button"
              disabled={busy || picking}
              className={method === 'password' ? 'active' : ''}
              aria-pressed={method === 'password'}
              onClick={() => {
                setMethod('password');
                setKey('');
                setKeyFile(null);
              }}
            >
              密码
            </button>
            <button
              type="button"
              disabled={busy || picking}
              className={method === 'key' ? 'active' : ''}
              aria-pressed={method === 'key'}
              onClick={() => {
                setMethod('key');
                setPassword('');
              }}
            >
              密钥
            </button>
          </div>
          {method === 'password' ? (
            <label>
              {creating ? '设置密码' : '对象库密码'}
              <input
                autoFocus={!onNameChange}
                disabled={busy || picking}
                type="password"
                autoComplete={creating || changing ? 'new-password' : 'current-password'}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>
          ) : (
            <>
              <label>
                密钥
                <input
                  type="password"
                  disabled={busy || picking}
                  autoComplete="off"
                  value={key}
                  onChange={(e) => {
                    setKey(e.target.value);
                    setKeyFile(null);
                  }}
                  placeholder="十六进制或 Base64 密钥"
                />
              </label>
              <button
                type="button"
                className="button secondary"
                disabled={busy || picking}
                onClick={() => {
                  setImportError('');
                  pickerBusy(true);
                  void repositoryApi
                    .importKey()
                    .then((k) => {
                      if (k) {
                        setKeyFile(k);
                        setKey('');
                      }
                    })
                    .catch((e) => setImportError(errorText(e)))
                    .finally(() => pickerBusy(false));
                }}
              >
                <KeyRound size={16} />
                {keyFile ? keyFile.name : '导入密钥文件'}
              </button>
              {(creating || changing) && (
                <button
                  type="button"
                  className="button secondary"
                  disabled={busy || picking}
                  onClick={() => {
                    setImportError('');
                    pickerBusy(true);
                    void repositoryApi
                      .generateKey()
                      .then((k) => {
                        if (k) {
                          setKeyFile(k);
                          setKey('');
                        }
                      })
                      .catch((e) => setImportError(errorText(e)))
                      .finally(() => pickerBusy(false));
                  }}
                >
                  <WandSparkles size={16} />
                  生成并保存新密钥
                </button>
              )}
            </>
          )}
          {(creating || changing) && method === 'password' && (
            <label>
              确认密码
              <input
                type="password"
                autoComplete="new-password"
                value={confirmation}
                onChange={(e) => setConfirmation(e.target.value)}
                required
                disabled={busy || picking}
              />
            </label>
          )}
          <label className="repository-remember">
            <input
              type="checkbox"
              disabled={busy || picking}
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            {method === 'key' ? '在本机保存密钥' : '记住密码'}
            <small>使用系统安全存储</small>
          </label>
        </>
      )}
      {(error || importError) && (
        <p className="repository-error" role="alert">
          {error || importError}
        </p>
      )}
      <div className="repository-unlock-actions">
        <button
          type="button"
          className="button quiet"
          disabled={busy || picking}
          onClick={onCancel}
        >
          {changing || onNameChange || managementName ? '取消' : '返回文件夹'}
        </button>
        {saved && !creating && (
          <button
            type="button"
            className="button secondary"
            disabled={busy || picking}
            onClick={() => onSubmit({}, false, true)}
          >
            使用已保存的解锁信息
          </button>
        )}
        <button
          className="button primary"
          disabled={
            busy || picking || (encrypted && !(method === 'password' ? password : key || keyFile))
          }
        >
          {busy ? (
            <Loader2 size={16} className="spin" />
          ) : encrypted ? (
            <LockKeyhole size={16} />
          ) : (
            <Box size={16} />
          )}{' '}
          {changing ? '保存解锁方式' : creating ? '创建对象库' : '解锁'}
        </button>
      </div>
    </form>
  );
}

export function RepositoryCreateDialog({
  parent,
  onClose,
  onCreated,
}: {
  parent: CloudFile;
  onClose: () => void;
  onCreated: (view: RepositoryView, name: string) => void;
}) {
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [pickerPending, setPickerPending] = useState(false);
  const [error, setError] = useState('');
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  return (
    <RepositoryDialog
      compact
      titleId="repository-create-title"
      busy={busy || pickerPending}
      onDismiss={onClose}
    >
      <UnlockFields
        creating
        name={name}
        onNameChange={setName}
        titleId="repository-create-title"
        busy={busy}
        error={error}
        saved={false}
        onCancel={onClose}
        onPickerPending={setPickerPending}
        onSubmit={(unlock, remember, _saved, encrypted) => {
          setBusy(true);
          setError('');
          void repositoryApi
            .createNamed(parent, name.trim(), !!encrypted, unlock, remember)
            .then((view) => {
              if (alive.current) onCreated(view, name.trim());
              else void repositoryApi.close(view.handle).catch(() => {});
            })
            .catch((e) => {
              if (alive.current) setError(errorText(e));
            })
            .finally(() => {
              if (alive.current) setBusy(false);
            });
        }}
      />
    </RepositoryDialog>
  );
}

export interface RepositoryControls {
  refresh: (directory?: string, force?: boolean) => Promise<RepositoryView | undefined>;
  acceptView: (view: RepositoryView) => void;
  upload: (mode?: 'files' | 'directory', paths?: string[]) => Promise<void>;
  manage: () => void;
}

export function RepositoryWorkspace({
  folder,
  probe,
  creating = false,
  initialView,
  initialDirectory,
  manage = false,
  active = true,
  onClose,
  onChanged,
  controls,
  onView,
  onInitialFileLocated,
  onBusy,
  onNotice,
  onQueued,
  fallbackFocus,
}: {
  folder: CloudFile;
  probe?: RepositoryProbe;
  creating?: boolean;
  initialView?: RepositoryView;
  initialDirectory?: string;
  manage?: boolean;
  active?: boolean;
  onClose: () => void;
  onChanged: () => void;
  controls?: Ref<RepositoryControls>;
  onView?: (view: RepositoryView | null) => void;
  onInitialFileLocated?: (
    repositoryId: string,
    item: RepositoryView['items'][number],
  ) => void;
  onBusy?: (busy: boolean) => void;
  onNotice?: (text: string, error: boolean) => void;
  onQueued?: (jobs: TransferJob[]) => void;
  fallbackFocus?: { readonly current: HTMLElement | null };
}) {
  const [view, setView] = useState<RepositoryView | null>(initialView ?? null);
  const [busy, setBusy] = useState(!initialView && !creating && probe?.kind === 'plain');
  const [managing, setManaging] = useState(false);
  const [keyPickerPending, setKeyPickerPending] = useState(false);
  const [changing, setChanging] = useState<'password' | 'key' | null>(null);
  const [managementError, setManagementError] = useState('');
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const running = useRef(false);
  const actionFocus = useRef<HTMLElement | null>(null);
  const workspace = useRef<HTMLElement>(null);
  const deferredFocus = useRef<{ target: HTMLElement | null } | null>(null);
  const [focusVersion, bumpFocus] = useState(0);
  const [plan, setPlan] = useState<RepositoryUploadPlan | null>(null);
  const [replace, setReplace] = useState(false);
  const [page, setPage] = useState(0);
  const handle = useRef<string | null>(initialView?.handle ?? null);
  const alive = useRef(true);
  const restoreAfterDialog = (target: HTMLElement | null) => {
    if (!alive.current) return;
    deferredFocus.current = { target };
    bumpFocus((value) => value + 1);
  };
  useEffect(() => {
    if (busy || plan || managing || !active || !deferredFocus.current) return;
    // The toolbar belongs to the parent now. Let its busy-state update commit
    // before returning focus to a trigger which was disabled during the dialog.
    const timer = setTimeout(() => {
      const target = deferredFocus.current?.target;
      const fallback =
        fallbackFocus?.current ??
        workspace.current?.querySelector<HTMLElement>('button:not(:disabled)');
      const next =
        target?.isConnected && !target.matches(':disabled') && !target.closest('[hidden],[inert]')
          ? target
          : fallback;
      if (next) {
        next.focus();
        deferredFocus.current = null;
      } else if (!target?.isConnected) deferredFocus.current = null;
    }, 0);
    return () => clearTimeout(timer);
  }, [busy, plan, managing, active, focusVersion]);
  const accept = (v: RepositoryView) => {
    if (!alive.current) {
      void repositoryApi.close(v.handle).catch(() => {});
      return;
    }
    handle.current = v.handle;
    setView(v);
    setError('');
  };
  const openedSession = (id: string) => {
    if (!alive.current) void repositoryApi.close(id).catch(() => {});
    else handle.current = id;
  };
  const exit = () => {
    if (keyPickerPending) return;
    const closing = handle.current;
    handle.current = null;
    if (closing) void repositoryApi.close(closing).catch(() => {});
    onClose();
  };
  const refresh = useCallback(
    async (directory?: string, force = directory === undefined) => {
      if (!view) return;
      const next = await repositoryApi.list(view.handle, directory ?? view.directory, force);
      accept(next);
      return next;
    },
    [view],
  );
  async function run(f: () => Promise<unknown>) {
    if (running.current) return;
    if (
      !plan &&
      document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
    )
      actionFocus.current = document.activeElement;
    running.current = true;
    setBusy(true);
    setError('');
    setMessage('');
    try {
      await f();
    } catch (e) {
      if (alive.current) setError(errorText(e));
    } finally {
      running.current = false;
      if (alive.current) setBusy(false);
    }
  }
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      const closing = handle.current;
      // React's development remount reuses the same refs. Close only if the
      // workspace remains unmounted, including an already-created session.
      queueMicrotask(() => {
        if (!alive.current && handle.current === closing) {
          if (closing) void repositoryApi.close(closing).catch(() => {});
          handle.current = null;
        }
      });
    };
  }, []);
  const acceptOpened = async (base: RepositoryView) => {
    if (!initialDirectory) {
      accept(base);
      return;
    }
    try {
      const sub = await repositoryApi.list(base.handle, initialDirectory, false);
      if (sub.items.length > 0 || sub.conflicts.length > 0) {
        accept(sub);
        return;
      }
      const slash = initialDirectory.lastIndexOf('/');
      const parentDir = slash < 0 ? '' : initialDirectory.slice(0, slash);
      const leaf = slash < 0 ? initialDirectory : initialDirectory.slice(slash + 1);
      const parentView = parentDir
        ? await repositoryApi.list(base.handle, parentDir, false)
        : base;
      const match = parentView.items.find((item) => item.name === leaf);
      if (match) {
        accept(parentView);
        onInitialFileLocated?.(parentView.repositoryId, match);
        return;
      }
      accept(base);
      onNotice?.(`路径不存在：${initialDirectory}`, true);
    } catch (e) {
      accept(base);
      onNotice?.(errorText(e), true);
    }
  };
  useEffect(() => {
    if (initialView || creating || probe?.kind !== 'plain') return;
    let disposed = false;
    setBusy(true);
    setError('');
    void repositoryApi
      .open(folder.id, probe.repositoryId!, {}, false, false, openedSession)
      .then(async (value) => {
        if (disposed) void repositoryApi.close(value.handle).catch(() => {});
        else await acceptOpened(value);
      })
      .catch((e) => {
        if (!disposed) setError(errorText(e));
      })
      .finally(() => {
        if (!disposed) setBusy(false);
      });
    return () => {
      disposed = true;
    };
  }, [folder.id, creating, probe?.kind, probe?.repositoryId]);
  useEffect(() => {
    const back = (e: Event) => {
      if (!active) return;
      e.preventDefault();
      if (busy || keyPickerPending) return;
      if (manage) onClose();
      else if (managing) {
        if (changing) setChanging(null);
        else setManaging(false);
      } else if (plan) setPlan(null);
      else if (view?.directory)
        void run(() => refresh(view.directory.split('/').slice(0, -1).join('/')));
      else onClose();
    };
    window.addEventListener('cmcloud-repository-back', back);
    return () => window.removeEventListener('cmcloud-repository-back', back);
  }, [active, busy, plan, view, onClose, refresh, manage, managing, changing, keyPickerPending]);
  const submitUnlock = (
    u: RepositoryUnlock,
    remember: boolean,
    useSaved = false,
    encrypted = true,
  ) =>
    void run(async () => {
      const opened = creating
        ? await repositoryApi.create(folder.id, encrypted, u, remember)
        : await repositoryApi.open(
            folder.id,
            probe!.repositoryId!,
            u,
            useSaved,
            remember,
            openedSession,
          );
      await acceptOpened(opened);
      onChanged();
    });
  useEffect(() => {
    if (!manage) onView?.(view);
  }, [manage, view, onView]);
  useEffect(() => {
    if (view) void repositoryApi.labelCache(view.handle, folder.name).catch(() => {});
  }, [view?.handle, folder.name]);
  useEffect(() => {
    onBusy?.(busy || keyPickerPending);
  }, [busy, keyPickerPending, onBusy]);
  useEffect(() => {
    if (error) onNotice?.(error, true);
  }, [error, onNotice]);
  useEffect(() => {
    if (message) onNotice?.(message, false);
  }, [message, onNotice]);
  useImperativeHandle(controls, () => ({
    refresh,
    acceptView: accept,
    manage: () => {
      if (!busy && view) {
        actionFocus.current = document.activeElement as HTMLElement;
        setManaging(true);
      }
    },
    upload: async (mode = 'files', paths) => {
      if (!view || busy || view.historical) return;
      actionFocus.current = document.activeElement as HTMLElement;
      await run(async () => {
        const next = await repositoryApi.plan(view.handle, view.directory, mode, paths);
        if (next) {
          setPlan(next);
          setReplace(false);
          setPage(0);
        }
      });
    },
  }));
  const hasReplace = plan?.items.some((i) => i.action.startsWith('replace-')) ?? false;
  const opening =
    !view &&
    (probe?.kind === 'plain' && !creating ? (
      <div className={manage ? 'repository-dialog' : 'repository-state'}>
        {manage && <h2 id="repository-manage-title">管理对象库</h2>}
        {busy ? (
          <>
            <p className={manage ? 'repository-callout' : undefined} role="status">
              <Loader2 className="spin" size={manage ? 18 : 24} />
              {manage ? `正在加载 ${folder.name}…` : '正在验证对象库并读取目录…'}
            </p>
            {!manage && <p>首次打开正在读取完整目录，已验证的目录会保存在本机以加快后续访问。</p>}
          </>
        ) : (
          <>
            <p role="alert" className="repository-error">
              {error || '正在检查对象库…'}
            </p>
            <button
              className="button secondary"
              onClick={() =>
                void run(async () =>
                  accept(
                    await repositoryApi.open(
                      folder.id,
                      probe.repositoryId!,
                      {},
                      false,
                      false,
                      openedSession,
                    ),
                  ),
                )
              }
            >
              <RefreshCw size={16} />
              重新读取
            </button>
          </>
        )}
        {(busy || manage) && (
          <button className="button quiet" onClick={exit}>
            {manage ? '取消' : '取消并返回'}
          </button>
        )}
      </div>
    ) : (
      <UnlockFields
        creating={creating}
        titleId={manage ? 'repository-manage-title' : undefined}
        managementName={manage ? folder.name : undefined}
        onSubmit={submitUnlock}
        busy={busy}
        error={error}
        saved={!!probe?.saved}
        onCancel={exit}
        onPickerPending={setKeyPickerPending}
      />
    ));
  const dismissManagement = () => {
    if (manage) exit();
    else {
      setManaging(false);
      setChanging(null);
    }
  };
  return (
    <section
      ref={workspace}
      className={manage || view ? 'repository-session' : 'workspace repository-opening'}
      hidden={!active}
      aria-label="AxuanStor 对象库"
    >
      {!manage && !view && (
        <header className="repository-heading">
          <button
            className="icon-button"
            aria-label="返回文件夹"
            disabled={keyPickerPending}
            onClick={exit}
          >
            <ArrowLeft size={20} />
          </button>
          <AxuanStorMark />
          <div>
            <h1>{folder.name}</h1>
            <p>{probe?.kind === 'encrypted' ? '加密对象库' : 'AxuanStor 对象库'}</p>
          </div>
        </header>
      )}
      {(manage || (managing && view)) && (
        <RepositoryDialog
          key={changing ?? 'summary'}
          compact
          titleId="repository-manage-title"
          focusKey={view ? 'management' : 'opening'}
          busy={busy || keyPickerPending}
          onDismiss={dismissManagement}
          onReturnFocus={manage ? undefined : restoreAfterDialog}
          returnFocus={actionFocus.current}
        >
          {!view ? (
            opening
          ) : changing ? (
            <UnlockFields
              key={changing}
              changing
              initialMethod={changing}
              titleId="repository-manage-title"
              busy={busy}
              error={managementError}
              saved={false}
              onPickerPending={setKeyPickerPending}
              onCancel={() => {
                setChanging(null);
                setManagementError('');
              }}
              onSubmit={(unlock, remember) => {
                if (!view.credentialRevision) return;
                void run(async () => {
                  try {
                    const result = await repositoryApi.changeUnlock(
                      view.handle,
                      view.credentialRevision!,
                      unlock,
                      remember,
                    );
                    accept(result.view);
                    setChanging(null);
                    setMessage(result.notice ?? '对象库解锁方式已更新。');
                    onChanged();
                  } catch (e) {
                    setManagementError(errorText(e));
                  }
                });
              }}
            />
          ) : (
            <div className="repository-management">
              <h2 id="repository-manage-title">管理对象库</h2>
              {repositoryDemo && <p className="repository-demo">示例数据</p>}
              <dl>
                <div>
                  <dt>名称</dt>
                  <dd>{folder.name}</dd>
                </div>
                <div>
                  <dt>类型</dt>
                  <dd>{view.encrypted ? '加密对象库' : '普通对象库'}</dd>
                </div>
                <div>
                  <dt>对象库 ID</dt>
                  <dd className="repository-id">{view.repositoryId}</dd>
                </div>
                {view.encrypted && (
                  <div>
                    <dt>解锁方式</dt>
                    <dd>
                      {(view.unlockMethods ?? [])
                        .map((m) => (m === 'password' ? '密码' : '密钥'))
                        .join('、') || '密码或密钥'}
                    </dd>
                  </div>
                )}
              </dl>
              {view.encrypted && view.canChangeUnlock && (
                <>
                  <div className="repository-management-actions">
                    <button
                      className="button secondary"
                      disabled={busy}
                      onClick={() => {
                        setManagementError('');
                        setChanging('password');
                      }}
                    >
                      <LockKeyhole size={16} />
                      修改密码
                    </button>
                    <button
                      className="button secondary"
                      disabled={busy}
                      onClick={() => {
                        setManagementError('');
                        setChanging('key');
                      }}
                    >
                      <KeyRound size={16} />
                      更换密钥
                    </button>
                  </div>
                  <p>
                    修改后请使用新凭据解锁。已解锁设备及持有旧密钥和历史副本的设备仍可能访问原有内容。
                  </p>
                </>
              )}
              {view.encrypted && !view.canChangeUnlock && (
                <p>
                  这个对象库使用早期的固定解锁格式。当前版本可继续读写；更换凭据需要先迁移对象库的解锁格式。
                </p>
              )}
              {view.saved && (
                <button
                  className="button quiet"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setManagementError('');
                      try {
                        await repositoryApi.forget(view.repositoryId);
                        setView({ ...view, saved: false });
                        setMessage('已清除此对象库在本机保存的解锁信息。');
                        onChanged();
                      } catch (e) {
                        setManagementError(errorText(e));
                      }
                    })
                  }
                >
                  清除本机保存的解锁信息
                </button>
              )}
              {managementError && (
                <p className="repository-error" role="alert">
                  {managementError}
                </p>
              )}
              {manage && message && <p role="status">{message}</p>}
              <div className="repository-unlock-actions">
                <button
                  className="button primary"
                  disabled={busy}
                  onClick={dismissManagement}
                >
                  完成
                </button>
              </div>
            </div>
          )}
        </RepositoryDialog>
      )}
      {!manage && !view && opening}
      {!manage && view && (
        <>
          {plan && (
            <RepositoryDialog
              titleId="repository-plan-title"
              onReturnFocus={restoreAfterDialog}
              returnFocus={actionFocus.current}
              busy={busy}
              onDismiss={() => setPlan(null)}
            >
              <section className="repository-dialog repository-plan">
                <div className="dialog-heading">
                  <h2 id="repository-plan-title">确认上传</h2>
                  <button
                    className="icon-button"
                    aria-label="关闭上传比对"
                    autoFocus
                    disabled={busy}
                    onClick={() => setPlan(null)}
                  >
                    <X size={18} />
                  </button>
                </div>
                <p>
                  {repositoryDemo ? '示例数据 · ' : ''}已比对 {plan.items.length} 个文件
                  {plan.directories ? `、${plan.directories} 个文件夹` : ''}、已有内容及本批重复项。
                  {plan.skipped ? `已跳过 ${plan.skipped} 个特殊项目。` : ''}
                </p>
                <dl className="repository-plan-summary">
                  <div>
                    <dt>逻辑文件</dt>
                    <dd>{formatBytes(plan.logicalBytes)}</dd>
                  </div>
                  <div>
                    <dt>新增内容</dt>
                    <dd>{plan.uniqueUploads} 份</dd>
                  </div>
                  <div>
                    <dt>需上传</dt>
                    <dd>{formatBytes(plan.uploadBytes)}</dd>
                  </div>
                </dl>
                <div className="repository-plan-items">
                  {plan.items.slice(page * 50, page * 50 + 50).map((item, i) => (
                    <div key={`${item.path}-${i}`}>
                      <span>{item.path}</span>
                      <small>{actionText(item.action)}</small>
                    </div>
                  ))}
                </div>
                {plan.items.length > 50 && (
                  <div className="repository-pagination">
                    <button
                      className="button quiet"
                      disabled={page === 0}
                      onClick={() => setPage(page - 1)}
                    >
                      <ArrowLeft size={15} />
                      上一页
                    </button>
                    <span>
                      {page + 1}/{Math.ceil(plan.items.length / 50)}
                    </span>
                    <button
                      className="button quiet"
                      disabled={(page + 1) * 50 >= plan.items.length}
                      onClick={() => setPage(page + 1)}
                    >
                      下一页
                      <ArrowRight size={15} />
                    </button>
                  </div>
                )}
                {hasReplace && (
                  <label className="repository-remember">
                    <input
                      type="checkbox"
                      checked={replace}
                      onChange={(e) => setReplace(e.target.checked)}
                    />
                    确认替换所列的已有路径
                  </label>
                )}
                {error && (
                  <p className="repository-error" role="alert">
                    {error}
                  </p>
                )}
                <div className="dialog-actions">
                  <button
                    className="button secondary"
                    disabled={busy}
                    onClick={() => setPlan(null)}
                  >
                    取消
                  </button>
                  <button
                    className="button primary"
                    disabled={busy || (hasReplace && !replace)}
                    onClick={() =>
                      void run(async () => {
                        const jobs = await repositoryApi.execute(view.handle, plan.id, replace);
                        onQueued?.(jobs);
                        setPlan(null);
                        setMessage('上传任务已加入传输队列');
                      })
                    }
                  >
                    {busy ? <Loader2 className="spin" size={16} /> : <Upload size={16} />}确认上传
                  </button>
                </div>
              </section>
            </RepositoryDialog>
          )}
        </>
      )}
    </section>
  );
}
