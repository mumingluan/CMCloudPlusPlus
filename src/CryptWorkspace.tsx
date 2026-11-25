import { useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import type { Ref } from 'react';
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  Copy,
  KeyRound,
  Loader2,
  LockKeyhole,
  ShieldCheck,
  Trash2,
  Unlink,
} from 'lucide-react';
import {
  cryptApi,
  cryptDemo,
  defaultCryptOptions,
  type CryptFolderOptions,
  type CryptFolderProbe,
  type CryptNameEncoding,
  type CryptNameEncryption,
  type CryptView,
} from './cryptApi';
import type { CloudFile, TransferJob } from './types';
import { errorText } from './api';
import { validName } from './utils';
import { RepositoryDialog } from './RepositoryDialog';
import './repository.css';

export function OpenListCryptMark() {
  return (
    <svg
      className="openlist-crypt-mark"
      role="img"
      aria-label="OpenList 加密文件夹"
      viewBox="0 0 16 16"
    >
      <rect x=".5" y=".5" width="15" height="15" rx="4" fill="currentColor" />
      <path
        d="M5.5 7V5.6a2.5 2.5 0 0 1 5 0V7"
        fill="none"
        stroke="white"
        strokeWidth="1.45"
        strokeLinecap="round"
      />
      <rect x="4.1" y="7" width="7.8" height="5.6" rx="1.5" fill="white" />
      <circle cx="8" cy="9.8" r=".95" fill="currentColor" />
    </svg>
  );
}

const encryptionModeLabels: Record<CryptNameEncryption, string> = {
  standard: 'Standard（AES-EME 标准加密）',
  obfuscate: 'Obfuscate（字符位移混淆）',
  off: 'Off（保留文件名，附加后缀）',
};

function CryptOptionFields({
  options,
  onChange,
  disabled,
  defaultExpanded = false,
}: {
  options: CryptFolderOptions;
  onChange: (next: CryptFolderOptions) => void;
  disabled: boolean;
  defaultExpanded?: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  return (
    <div className="crypt-options-box">
      <button
        type="button"
        className="crypt-options-toggle"
        aria-expanded={expanded}
        disabled={disabled}
        onClick={() => setExpanded((value) => !value)}
      >
        <span>
          OpenList 兼容参数（{options.filenameEncryption}
          {options.filenameEncryption === 'standard' ? ` · ${options.filenameEncoding}` : ''}
          {options.filenameEncryption === 'off' ? ` · ${options.encryptedSuffix || '.bin'}` : ''}）
        </span>
        {expanded ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
      </button>
      {expanded && (
        <div className="crypt-options-fields">
          <label>
            文件名加密方式 (filename_encryption)
            <select
              disabled={disabled}
              value={options.filenameEncryption}
              onChange={(e) =>
                onChange({
                  ...options,
                  filenameEncryption: e.target.value as CryptNameEncryption,
                })
              }
            >
              <option value="standard">standard（OpenList 默认 · AES-256-EME）</option>
              <option value="obfuscate">obfuscate（轻量混淆）</option>
              <option value="off">off（不加密文件名，仅加后缀）</option>
            </select>
          </label>
          {options.filenameEncryption === 'standard' && (
            <label>
              文件名编码格式 (filename_encoding)
              <select
                disabled={disabled}
                value={options.filenameEncoding}
                onChange={(e) =>
                  onChange({
                    ...options,
                    filenameEncoding: e.target.value as CryptNameEncoding,
                  })
                }
              >
                <option value="base64">base64（OpenList 默认 · URL 安全无填充）</option>
                <option value="base32">base32（rclone 默认 · 大小写不敏感）</option>
                <option value="base32768">base32768（短中文编码 · 适合深层长路径）</option>
              </select>
            </label>
          )}
          {options.filenameEncryption === 'off' && (
            <label>
              密文文件后缀 (encrypted_suffix)
              <input
                type="text"
                disabled={disabled}
                value={options.encryptedSuffix}
                placeholder=".bin（填 none 表示无后缀）"
                onChange={(e) => onChange({ ...options, encryptedSuffix: e.target.value })}
              />
            </label>
          )}
          {options.filenameEncryption !== 'off' && (
            <label className="repository-remember">
              <input
                type="checkbox"
                disabled={disabled}
                checked={options.directoryNameEncryption}
                onChange={(e) =>
                  onChange({ ...options, directoryNameEncryption: e.target.checked })
                }
              />
              加密子目录名称 (directory_name_encryption)
            </label>
          )}
          <label className="repository-remember">
            <input
              type="checkbox"
              disabled={disabled}
              checked={options.showHidden}
              onChange={(e) => onChange({ ...options, showHidden: e.target.checked })}
            />
            显示以 . 开头的隐藏文件 (show_hidden)
          </label>
        </div>
      )}
    </div>
  );
}

function CryptCredentialForm({
  mode,
  folderName,
  onFolderNameChange,
  initialOptions,
  saved,
  defaultExpandOptions = false,
  titleId,
  busy,
  error,
  onCancel,
  onSubmit,
}: {
  mode: 'create' | 'unlock' | 'edit';
  folderName?: string;
  onFolderNameChange?: (value: string) => void;
  initialOptions: CryptFolderOptions;
  saved: boolean;
  defaultExpandOptions?: boolean;
  titleId?: string;
  busy: boolean;
  error: string;
  onCancel: () => void;
  onSubmit: (payload: {
    password: string;
    salt: string;
    options: CryptFolderOptions;
    remember: boolean;
    useSaved?: boolean;
  }) => void;
}) {
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [salt, setSalt] = useState('');
  const [options, setOptions] = useState<CryptFolderOptions>(initialOptions);
  const [remember, setRemember] = useState(saved);
  const [localError, setLocalError] = useState('');

  const needConfirm = mode === 'create' && !password.startsWith('___Obfuscated___');
  const passwordRequired = mode !== 'edit';

  return (
    <form
      className="repository-unlock crypt-form"
      aria-busy={busy}
      onSubmit={(e) => {
        e.preventDefault();
        if (busy) return;
        if (onFolderNameChange && !validName(folderName ?? '')) {
          setLocalError('请输入有效的加密文件夹名称。');
          return;
        }
        if (passwordRequired && !password) {
          setLocalError('请输入加密文件夹密码。');
          return;
        }
        if (needConfirm && password !== confirmation) {
          setLocalError('两次输入的密码不一致，请重新确认。');
          return;
        }
        setLocalError('');
        onSubmit({
          password,
          salt,
          options,
          remember,
          useSaved: false,
        });
      }}
    >
      <div className="repository-unlock-icon crypt-unlock-icon">
        <ShieldCheck size={28} />
      </div>
      <h2 id={titleId}>
        {mode === 'create'
          ? '新建加密文件夹'
          : mode === 'edit'
            ? '修改口令与兼容参数'
            : '解锁加密文件夹'}
      </h2>
      <p>
        {mode === 'create'
          ? '与 OpenList Crypt / rclone crypt 1:1 完全兼容，文件在上传与下载时自动流式加解密。'
          : mode === 'edit'
            ? '更新本机关联的 OpenList 加密参数或保存的口令。留空密码则仅更新参数。'
            : `输入口令后在本地实时解密浏览「${folderName ?? '加密文件夹'}」。支持直接粘贴 OpenList 的 ___Obfuscated___ 口令。`}
      </p>
      {cryptDemo && <p className="repository-demo">界面演示 · 口令为 demo</p>}
      {onFolderNameChange && (
        <label>
          文件夹名称
          <input
            autoFocus
            required
            maxLength={255}
            disabled={busy}
            value={folderName ?? ''}
            onChange={(e) => onFolderNameChange(e.target.value)}
            placeholder="例如：私密备份"
          />
        </label>
      )}
      <label>
        {mode === 'edit' ? '新密码 (password，留空保持不变)' : '加密密码 (password)'}
        <input
          autoFocus={!onFolderNameChange}
          type="password"
          autoComplete={mode === 'create' ? 'new-password' : 'current-password'}
          required={passwordRequired}
          disabled={busy}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder={
            mode === 'edit' ? '如需更新口令请输入' : '明文密码或 ___Obfuscated___ 口令'
          }
        />
      </label>
      {needConfirm && (
        <label>
          确认密码
          <input
            type="password"
            autoComplete="new-password"
            required
            disabled={busy}
            value={confirmation}
            onChange={(e) => setConfirmation(e.target.value)}
          />
        </label>
      )}
      <label>
        加密盐 (salt / password2，可选)
        <input
          type="password"
          autoComplete="off"
          disabled={busy}
          value={salt}
          onChange={(e) => setSalt(e.target.value)}
          placeholder="未设置可留空（使用 OpenList 默认盐）"
        />
      </label>
      <CryptOptionFields
        options={options}
        onChange={setOptions}
        disabled={busy}
        defaultExpanded={defaultExpandOptions}
      />
      <label className="repository-remember">
        <input
          type="checkbox"
          disabled={busy}
          checked={remember}
          onChange={(e) => setRemember(e.target.checked)}
        />
        在此设备记住口令
        <small>使用系统安全存储</small>
      </label>
      {(error || localError) && (
        <p className="repository-error" role="alert">
          {error || localError}
        </p>
      )}
      <div className="repository-unlock-actions">
        <button type="button" className="button quiet" disabled={busy} onClick={onCancel}>
          {mode === 'unlock' ? '返回文件夹' : '取消'}
        </button>
        {mode === 'unlock' && saved && (
          <button
            type="button"
            className="button secondary"
            disabled={busy}
            onClick={() =>
              onSubmit({
                password: '',
                salt: '',
                options,
                remember: true,
                useSaved: true,
              })
            }
          >
            使用已保存口令
          </button>
        )}
        <button
          className="button primary"
          disabled={busy || (passwordRequired && !password)}
        >
          {busy ? <Loader2 size={16} className="spin" /> : <LockKeyhole size={16} />}{' '}
          {mode === 'create' ? '创建加密文件夹' : mode === 'edit' ? '保存设置' : '解锁打开'}
        </button>
      </div>
    </form>
  );
}

export function CryptCreateDialog({
  parent,
  onClose,
  onCreated,
}: {
  parent: CloudFile;
  onClose: () => void;
  onCreated: (view: CryptView, name: string) => void;
}) {
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
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
      titleId="crypt-create-title"
      busy={busy}
      onDismiss={onClose}
    >
      <CryptCredentialForm
        mode="create"
        folderName={name}
        onFolderNameChange={setName}
        initialOptions={defaultCryptOptions}
        saved={false}
        titleId="crypt-create-title"
        busy={busy}
        error={error}
        onCancel={onClose}
        onSubmit={({ password, salt, options, remember }) => {
          setBusy(true);
          setError('');
          const trimmed = name.trim();
          void cryptApi
            .create(parent, trimmed, password, salt, options, remember)
            .then((view) => {
              if (alive.current) onCreated(view, trimmed);
              else void cryptApi.close(view.handle).catch(() => {});
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

export interface CryptControls {
  refresh: (directory?: string, force?: boolean) => Promise<CryptView | undefined>;
  acceptView: (view: CryptView) => void;
  upload: (
    mode?: 'files' | 'directory',
    paths?: string[],
  ) => Promise<{ queued: number; skipped: number } | undefined>;
  manage: () => void;
}

export function CryptWorkspace({
  folder,
  probe,
  initialView,
  initialDirectory,
  manage = false,
  active = true,
  onClose,
  onChanged,
  onUnbound,
  controls,
  onView,
  onInitialFileLocated,
  onBusy,
  onNotice,
  onQueued,
  fallbackFocus,
}: {
  folder: CloudFile;
  probe?: CryptFolderProbe;
  initialView?: CryptView;
  initialDirectory?: string;
  manage?: boolean;
  active?: boolean;
  onClose: () => void;
  onChanged: () => void;
  onUnbound?: (folderId: string) => void;
  controls?: Ref<CryptControls>;
  onView?: (view: CryptView | null) => void;
  onInitialFileLocated?: (folderId: string, item: CryptView['items'][number]) => void;
  onBusy?: (busy: boolean) => void;
  onNotice?: (text: string, error: boolean) => void;
  onQueued?: (jobs: TransferJob[]) => void;
  fallbackFocus?: { readonly current: HTMLElement | null };
}) {
  const [view, setView] = useState<CryptView | null>(initialView ?? null);
  const [autoOpening, setAutoOpening] = useState(!initialView && !manage && !!probe?.saved);
  const [busy, setBusy] = useState(!initialView && !manage && !!probe?.saved);
  const [managing, setManaging] = useState(false);
  const [editingSettings, setEditingSettings] = useState(false);
  const [error, setError] = useState('');
  const [managementError, setManagementError] = useState('');
  const [message, setMessage] = useState('');
  const [localProbe, setLocalProbe] = useState<CryptFolderProbe | undefined>(probe);
  const running = useRef(false);
  const actionFocus = useRef<HTMLElement | null>(null);
  const workspace = useRef<HTMLElement>(null);
  const deferredFocus = useRef<{ target: HTMLElement | null } | null>(null);
  const [focusVersion, bumpFocus] = useState(0);
  const handle = useRef<string | null>(initialView?.handle ?? null);
  const alive = useRef(true);

  useEffect(() => {
    setLocalProbe(probe);
  }, [probe]);

  const restoreAfterDialog = (target: HTMLElement | null) => {
    if (!alive.current) return;
    deferredFocus.current = { target };
    bumpFocus((value) => value + 1);
  };

  useEffect(() => {
    if (busy || managing || !active || !deferredFocus.current) return;
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
  }, [busy, managing, active, focusVersion]);

  const accept = useCallback((next: CryptView) => {
    if (!alive.current) {
      void cryptApi.close(next.handle).catch(() => {});
      return;
    }
    if (handle.current && handle.current !== next.handle) {
      void cryptApi.close(handle.current).catch(() => {});
    }
    handle.current = next.handle;
    setView(next);
    setError('');
  }, []);

  const exit = () => {
    const closing = handle.current;
    handle.current = null;
    if (closing) void cryptApi.close(closing).catch(() => {});
    onClose();
  };

  const refresh = useCallback(
    async (directory?: string, force = directory === undefined) => {
      if (!view) return;
      const next = await cryptApi.list(view.handle, directory ?? view.directory, force);
      accept(next);
      return next;
    },
    [view, accept],
  );

  async function run(f: () => Promise<unknown>) {
    if (running.current) return;
    if (
      document.activeElement instanceof HTMLElement &&
      document.activeElement !== document.body
    ) {
      actionFocus.current = document.activeElement;
    }
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
      queueMicrotask(() => {
        if (!alive.current && handle.current === closing) {
          if (closing) void cryptApi.close(closing).catch(() => {});
          handle.current = null;
        }
      });
    };
  }, []);

  const acceptOpened = useCallback(
    async (base: CryptView) => {
      if (!initialDirectory) {
        accept(base);
        return;
      }
      try {
        const sub = await cryptApi.list(base.handle, initialDirectory, false);
        accept(sub);
      } catch {
        const slash = initialDirectory.lastIndexOf('/');
        const parentDir = slash < 0 ? '' : initialDirectory.slice(0, slash);
        const leaf = slash < 0 ? initialDirectory : initialDirectory.slice(slash + 1);
        try {
          const parentView = parentDir
            ? await cryptApi.list(base.handle, parentDir, false)
            : base;
          const match = parentView.items.find((item) => item.name === leaf);
          if (match) {
            accept(parentView);
            onInitialFileLocated?.(parentView.folderId, match);
            return;
          }
        } catch {
          /* Fall back to root view with path error notice below. */
        }
        accept(base);
        onNotice?.(`路径不存在：${initialDirectory}`, true);
      }
    },
    [accept, initialDirectory, onInitialFileLocated, onNotice],
  );

  useEffect(() => {
    if (initialView || manage || !probe?.saved) return;
    let disposed = false;
    setAutoOpening(true);
    setBusy(true);
    setError('');
    void cryptApi
      .open(folder, {
        options: probe.options ?? undefined,
        useSaved: true,
        remember: true,
      })
      .then(async (opened) => {
        if (disposed) void cryptApi.close(opened.handle).catch(() => {});
        else await acceptOpened(opened);
      })
      .catch((e) => {
        if (!disposed) setError(errorText(e));
      })
      .finally(() => {
        if (!disposed) {
          setAutoOpening(false);
          setBusy(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [folder.id]);

  useEffect(() => {
    const back = (e: Event) => {
      if (!active) return;
      e.preventDefault();
      if (busy) return;
      if (manage) onClose();
      else if (managing) {
        if (editingSettings) setEditingSettings(false);
        else setManaging(false);
      } else if (view?.directory) {
        void run(() => refresh(view.directory.split('/').slice(0, -1).join('/'), false));
      } else onClose();
    };
    window.addEventListener('cmcloud-crypt-back', back);
    return () => window.removeEventListener('cmcloud-crypt-back', back);
  }, [active, busy, manage, managing, editingSettings, view, onClose, refresh]);

  useEffect(() => {
    if (!manage) onView?.(view);
  }, [manage, view, onView]);

  useEffect(() => {
    onBusy?.(busy);
  }, [busy, onBusy]);

  useEffect(() => {
    if (error && view) onNotice?.(error, true);
  }, [error, view, onNotice]);

  useEffect(() => {
    if (message) onNotice?.(message, false);
  }, [message, onNotice]);

  useImperativeHandle(controls, () => ({
    refresh,
    acceptView: accept,
    manage: () => {
      if (!busy) {
        actionFocus.current = document.activeElement as HTMLElement;
        setManagementError('');
        setEditingSettings(false);
        setManaging(true);
      }
    },
    upload: async (mode = 'files', paths) => {
      if (!view || busy) return;
      if (mode === 'files' && !paths) {
        const jobs = await cryptApi.queueUploads(view.handle, view.directory);
        if (jobs.length) onQueued?.(jobs);
        return { queued: jobs.length, skipped: 0 };
      }
      return cryptApi.queueUploadDirectory(view.handle, view.directory, paths);
    },
  }));

  const currentOptions = view?.options ?? localProbe?.options ?? defaultCryptOptions;
  const currentSaved = view ? view.saved : !!localProbe?.saved;

  const dismissManagement = () => {
    if (manage) exit();
    else {
      setManaging(false);
      setEditingSettings(false);
    }
  };

  const copyOpenListConfig = () => {
    const summary = [
      `filename_encryption = ${currentOptions.filenameEncryption}`,
      `directory_name_encryption = ${currentOptions.directoryNameEncryption}`,
      `filename_encoding = ${currentOptions.filenameEncoding}`,
      `encrypted_suffix = ${currentOptions.encryptedSuffix || '.bin'}`,
      `show_hidden = ${currentOptions.showHidden}`,
    ].join('\n');
    void navigator.clipboard
      .writeText(summary)
      .then(() => setMessage('OpenList Crypt 参数已复制到剪贴板。'))
      .catch((e) => setManagementError(errorText(e)));
  };

  const openingContent = autoOpening ? (
    <div className="repository-state">
      <p className="repository-callout" role="status">
        <Loader2 className="spin" size={22} />
        正在解锁并读取加密文件夹…
      </p>
      <button className="button quiet" onClick={exit}>
        取消并返回
      </button>
    </div>
  ) : (
    <CryptCredentialForm
      mode="unlock"
      folderName={folder.name}
      initialOptions={currentOptions}
      saved={!!localProbe?.saved}
      defaultExpandOptions={!localProbe?.known}
      busy={busy}
      error={error}
      onCancel={exit}
      onSubmit={({ password, salt, options, remember, useSaved }) =>
        void run(async () => {
          const opened = await cryptApi.open(folder, {
            password,
            salt,
            options,
            remember,
            useSaved,
          });
          await acceptOpened(opened);
          onChanged();
        })
      }
    />
  );

  return (
    <section
      ref={workspace}
      className={manage || view ? 'repository-session' : 'workspace repository-opening'}
      hidden={!active}
      aria-label="OpenList 加密文件夹"
    >
      {!manage && !view && (
        <header className="repository-heading">
          <button
            className="icon-button"
            aria-label="返回文件夹"
            disabled={busy && !autoOpening}
            onClick={exit}
          >
            <ArrowLeft size={20} />
          </button>
          <OpenListCryptMark />
          <div>
            <h1>{folder.name}</h1>
            <p>OpenList 兼容加密文件夹</p>
          </div>
        </header>
      )}
      {(manage || (managing && view)) && (
        <RepositoryDialog
          key={editingSettings ? 'edit' : 'summary'}
          compact
          titleId="crypt-manage-title"
          focusKey={editingSettings ? 'edit' : 'summary'}
          busy={busy}
          onDismiss={dismissManagement}
          onReturnFocus={manage ? undefined : restoreAfterDialog}
          returnFocus={actionFocus.current}
        >
          {editingSettings ? (
            <CryptCredentialForm
              mode="edit"
              initialOptions={currentOptions}
              saved={currentSaved}
              defaultExpandOptions
              titleId="crypt-manage-title"
              busy={busy}
              error={managementError}
              onCancel={() => {
                setEditingSettings(false);
                setManagementError('');
              }}
              onSubmit={({ password, salt, options, remember }) =>
                void run(async () => {
                  try {
                    const updated = await cryptApi.updateSettings(folder.id, {
                      password: password || undefined,
                      salt: password ? salt : undefined,
                      options,
                      remember,
                    });
                    setLocalProbe(updated);
                    if (view) {
                      if (password || updated.saved) {
                        const reopened = await cryptApi.open(folder, {
                          password: password || undefined,
                          salt: password ? salt : undefined,
                          options,
                          useSaved: !password && updated.saved,
                          remember,
                        });
                        const targetView = view.directory
                          ? await cryptApi
                              .list(reopened.handle, view.directory, true)
                              .catch(() => reopened)
                          : reopened;
                        accept(targetView);
                      } else {
                        setView({ ...view, options, saved: updated.saved });
                      }
                    }
                    setEditingSettings(false);
                    setMessage('加密文件夹设置已保存。');
                    onChanged();
                  } catch (e) {
                    setManagementError(errorText(e));
                  }
                })
              }
            />
          ) : (
            <div className="repository-management">
              <h2 id="crypt-manage-title">加密文件夹设置</h2>
              {cryptDemo && <p className="repository-demo">示例数据</p>}
              <dl>
                <div>
                  <dt>名称</dt>
                  <dd>{folder.name}</dd>
                </div>
                <div>
                  <dt>加密格式</dt>
                  <dd>OpenList / rclone Crypt（XSalsa20-Poly1305）</dd>
                </div>
                <div>
                  <dt>文件名模式</dt>
                  <dd>{encryptionModeLabels[currentOptions.filenameEncryption]}</dd>
                </div>
                {currentOptions.filenameEncryption === 'standard' && (
                  <div>
                    <dt>名称编码</dt>
                    <dd>{currentOptions.filenameEncoding}</dd>
                  </div>
                )}
                {currentOptions.filenameEncryption !== 'off' ? (
                  <div>
                    <dt>子目录加密</dt>
                    <dd>{currentOptions.directoryNameEncryption ? '开启' : '关闭'}</dd>
                  </div>
                ) : (
                  <div>
                    <dt>密文后缀</dt>
                    <dd>{currentOptions.encryptedSuffix || '.bin'}</dd>
                  </div>
                )}
                <div>
                  <dt>口令存储</dt>
                  <dd>{currentSaved ? '已保存在本机系统安全存储' : '仅当前会话有效'}</dd>
                </div>
              </dl>
              <div className="repository-management-actions">
                <button
                  className="button secondary"
                  disabled={busy}
                  onClick={() => {
                    setManagementError('');
                    setEditingSettings(true);
                  }}
                >
                  <KeyRound size={16} />
                  修改口令或参数
                </button>
                <button className="button secondary" disabled={busy} onClick={copyOpenListConfig}>
                  <Copy size={16} />
                  复制 OpenList 参数
                </button>
              </div>
              {currentSaved && (
                <button
                  className="button quiet"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      setManagementError('');
                      try {
                        const updated = await cryptApi.updateSettings(folder.id, {
                          options: currentOptions,
                          remember: false,
                        });
                        setLocalProbe(updated);
                        if (view) setView({ ...view, saved: false });
                        setMessage('已清除此设备保存的加密文件夹口令。');
                        onChanged();
                      } catch (e) {
                        setManagementError(errorText(e));
                      }
                    })
                  }
                >
                  <Trash2 size={15} />
                  清除本机保存的口令
                </button>
              )}
              <button
                className="button quiet danger"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    setManagementError('');
                    try {
                      await cryptApi.updateSettings(folder.id, {
                        remember: false,
                        forget: true,
                      });
                      onChanged();
                      onUnbound?.(folder.id);
                      exit();
                    } catch (e) {
                      setManagementError(errorText(e));
                    }
                  })
                }
              >
                <Unlink size={15} />
                解除加密文件夹关联（保留云端密文）
              </button>
              {managementError && (
                <p className="repository-error" role="alert">
                  {managementError}
                </p>
              )}
              {manage && message && <p role="status">{message}</p>}
              <div className="repository-unlock-actions">
                <button className="button primary" disabled={busy} onClick={dismissManagement}>
                  完成
                </button>
              </div>
            </div>
          )}
        </RepositoryDialog>
      )}
      {!manage && !view && openingContent}
    </section>
  );
}
