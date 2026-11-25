import { useEffect, useRef, useState } from 'react';
import { Database, KeyRound, Loader2, RefreshCw, Trash2, X } from 'lucide-react';
import { RepositoryDialog } from './RepositoryDialog';
import { repositoryApi, repositoryDemo } from './repositoryApi';
import type { RepositoryCache } from './repositoryApi';
import { native, errorText } from './api';
import { formatBytes } from './utils';

export function RepositorySettings({ onForgotten }: { onForgotten?: () => void }) {
  const [cachesOpen, setCachesOpen] = useState(false);
  const [caches, setCaches] = useState<RepositoryCache[]>([]);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState('');
  const [error, setError] = useState('');
  const [message, setMessage] = useState('');
  const [confirmation, setConfirmation] = useState<0 | 1 | 2>(0);
  const [revision, setRevision] = useState(0);
  const cacheButton = useRef<HTMLButtonElement>(null);
  const forgetButton = useRef<HTMLButtonElement>(null);
  const submitting = useRef(false);
  useEffect(() => {
    if (!cachesOpen) return;
    let cancelled = false;
    setLoading(true);
    setError('');
    repositoryApi
      .caches()
      .then((rows) => {
        if (!cancelled) setCaches(rows);
      })
      .catch((reason) => {
        if (!cancelled) setError(errorText(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [cachesOpen, revision]);
  return (
    <section className="settings-group" aria-labelledby="repository-settings-title">
      <h2 id="repository-settings-title">AxuanStor 对象库</h2>
      <div className="setting-row">
        <div>
          <h3>本机对象库清单缓存</h3>
          <p>查看各对象库占用的缓存空间，按需删除。</p>
        </div>
        <button
          ref={cacheButton}
          className="button secondary"
          aria-label="管理本机对象库清单缓存"
          disabled={!native && !repositoryDemo}
          onClick={() => setCachesOpen(true)}
        >
          <Database size={16} />
          管理缓存
        </button>
      </div>
      <div className="setting-row">
        <div>
          <h3>本机保存的对象库凭据</h3>
          <p>管理保存的对象库密码与密钥。</p>
        </div>
        <button
          ref={forgetButton}
          className="button quiet danger"
          aria-label="删除本机全部对象库凭据存储"
          disabled={!native && !repositoryDemo}
          onClick={() => {
            setError('');
            setConfirmation(1);
          }}
        >
          <KeyRound size={16} />
          删除全部凭据
        </button>
      </div>
      {message && (
        <p className="settings-repository-message" role="status">
          {message}
        </p>
      )}
      {cachesOpen && (
        <RepositoryDialog
          titleId="repository-caches-title"
          returnFocus={cacheButton.current}
          busy={!!pending}
          onDismiss={() => setCachesOpen(false)}
        >
          <div className="repository-cache-dialog">
            <div className="dialog-heading">
              <h2 id="repository-caches-title">本机对象库清单缓存</h2>
              <button
                className="icon-button"
                aria-label="关闭缓存管理"
                disabled={!!pending}
                onClick={() => setCachesOpen(false)}
              >
                <X size={19} />
              </button>
            </div>
            {repositoryDemo && <p>示例数据</p>}
            {loading && (
              <p className="repository-callout" role="status">
                <Loader2 size={16} className="spin" />
                正在读取缓存…
              </p>
            )}
            {error && (
              <p className="repository-error" role="alert">
                {error}
              </p>
            )}
            {!loading && !error && caches.length === 0 && (
              <p className="repository-cache-empty">还没有对象库清单缓存。</p>
            )}
            <ul className="repository-cache-list">
              {caches.map((row) => (
                <li key={row.id}>
                  <div>
                    <strong>{row.name}</strong>
                    <span className="repository-cache-id">{row.repositoryId}</span>
                    <span>
                      {formatBytes(row.bytes)}
                      {row.busy ? ' · 使用中，请先退出对象库' : ''}
                    </span>
                  </div>
                  <button
                    className="button quiet danger"
                    aria-label={`删除 ${row.name} 的缓存`}
                    disabled={row.busy || !!pending || loading}
                    onClick={() => {
                      setPending(row.id);
                      setError('');
                      repositoryApi
                        .deleteCache(row.id)
                        .then(() => setRevision((value) => value + 1))
                        .catch((reason) => setError(errorText(reason)))
                        .finally(() => setPending(''));
                    }}
                  >
                    {pending === row.id ? (
                      <Loader2 size={16} className="spin" />
                    ) : (
                      <Trash2 size={16} />
                    )}
                    删除缓存
                  </button>
                </li>
              ))}
            </ul>
            <footer className="repository-dialog-actions">
              <button
                className="button quiet"
                disabled={loading || !!pending}
                onClick={() => setRevision((value) => value + 1)}
              >
                <RefreshCw size={16} />
                刷新
              </button>
              <button
                className="button secondary"
                disabled={!!pending}
                onClick={() => setCachesOpen(false)}
              >
                关闭
              </button>
            </footer>
          </div>
        </RepositoryDialog>
      )}
      {confirmation > 0 && (
        <RepositoryDialog
          key={confirmation}
          compact
          titleId="repository-forget-title"
          returnFocus={forgetButton.current}
          busy={pending === 'credentials'}
          onDismiss={() => setConfirmation(0)}
        >
          <div className="repository-dialog">
            <h2 id="repository-forget-title">
              {confirmation === 1 ? '删除本机保存的凭据？' : '再次确认删除全部凭据'}
            </h2>
            <p>
              {confirmation === 1
                ? '将移除此设备保存的对象库密码和密钥。已打开的对象库会退出，云盘账号仍保留登录。'
                : '之后打开加密对象库时，需要重新输入密码或密钥。请确认你已妥善保管这些信息。'}
            </p>
            {error && (
              <p className="repository-error" role="alert">
                {error}
              </p>
            )}
            <div className="repository-dialog-actions">
              <button
                className="button secondary"
                disabled={!!pending}
                onClick={() => setConfirmation(0)}
              >
                取消
              </button>
              <button
                className="button destructive"
                disabled={!!pending}
                onClick={() => {
                  if (submitting.current) return;
                  if (confirmation === 1) {
                    setConfirmation(2);
                    return;
                  }
                  submitting.current = true;
                  setPending('credentials');
                  setError('');
                  repositoryApi
                    .forgetAll()
                    .then(() => {
                      setConfirmation(0);
                      setMessage('已删除本机保存的 AxuanStor 凭据');
                      onForgotten?.();
                    })
                    .catch((reason) => setError(errorText(reason)))
                    .finally(() => {
                      submitting.current = false;
                      setPending('');
                    });
                }}
              >
                {pending ? <Loader2 size={16} className="spin" /> : null}
                {confirmation === 1 ? '继续确认' : '确认删除全部凭据'}
              </button>
            </div>
          </div>
        </RepositoryDialog>
      )}
    </section>
  );
}
