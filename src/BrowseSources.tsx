import { useEffect, useId, useRef, useState } from 'react';
import { Link, Loader2, MoreHorizontal, Plus, Trash2, Users, X } from 'lucide-react';
import { api, errorText, native } from './api';
import { FileMenuDialog } from './FileMenuDialog';
import { FileContextMenu } from './FileContextMenu';
import { RepositoryDialog } from './RepositoryDialog';
import type { BrowseSource } from './types';

const choices = [
  { kind: 'group', label: '添加群组ID', icon: Users },
  { kind: 'share', label: '添加分享链接', icon: Link },
] as const;

export function BrowseSources({ accountId, smallScreen, onOpen, onConnect, alert }: {
  accountId?: string;
  smallScreen: boolean;
  onOpen: (source: BrowseSource) => void;
  onConnect: () => void;
  alert: (message: string, error?: boolean) => void;
}) {
  const [sources, setSources] = useState<BrowseSource[]>([]);
  const [loading, setLoading] = useState(native && !!accountId);
  const [loadError, setLoadError] = useState('');
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [menu, setMenu] = useState(false);
  const [adding, setAdding] = useState<BrowseSource['kind'] | null>(null);
  const [sourceMenu, setSourceMenu] = useState<{ source: BrowseSource; x: number; y: number } | null>(null);
  const [removing, setRemoving] = useState(false);
  useEffect(() => {
    if (!native || !accountId) return;
    let disposed = false;
    void api.browseSources(accountId)
      .then((items) => { if (!disposed) setSources(items); })
      .catch((error) => { if (!disposed) setLoadError(errorText(error)); })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [accountId, loadAttempt]);

  const removeSource = async (source: BrowseSource) => {
    if (!accountId || removing) return;
    setRemoving(true);
    try {
      await api.removeBrowseSource(accountId, source.id);
      setSources((current) => current.filter((entry) => entry.id !== source.id));
    } catch (error) {
      alert(errorText(error), true);
    } finally {
      setRemoving(false);
    }
  };

  const addActions = (inMenu: boolean) => choices.map(({ kind, label, icon: Icon }) => (
    <button key={kind} type="button" className={inMenu ? 'menu-item' : 'button secondary'}
      onClick={() => {
        setMenu(false);
        if (accountId) setAdding(kind);
        else onConnect();
      }}>
      <Icon size={18} />
      {label}
    </button>
  ));
  return (
    <section className="workspace files-workspace browse-sources" aria-label="群组与分享">
      {loading ? (
        <div className="restoring-session" role="status">
          <Loader2 size={22} className="spin" />
          正在加载群组与分享…
        </div>
      ) : loadError ? (
        <div className="empty-folder" role="alert">
          <p>{loadError}</p>
          <button className="button secondary" onClick={() => {
            setLoadError('');
            setLoading(true);
            setLoadAttempt((attempt) => attempt + 1);
          }}>重试</button>
        </div>
      ) : sources.length === 0 ? (
        <div className="connect-empty">
          <div className="connect-content">
            <Users size={52} strokeWidth={1.2} />
            <h2>群组与分享</h2>
            <div className="connect-actions">{addActions(false)}</div>
          </div>
        </div>
      ) : (
        <>
          <header className="breadcrumb-bar browse-source-heading">
            <h2>群组与分享</h2>
            <button type="button" className="button quiet" aria-haspopup="dialog"
              aria-expanded={menu} onClick={() => setMenu(true)}>
              <Plus size={20} />
              新建
            </button>
          </header>
          <ul className="browse-source-list">
            {sources.map((source) => {
              const Icon = source.kind === 'group' ? Users : Link;
              return (
                <li key={source.id} onContextMenu={(event) => {
                  event.preventDefault();
                  if (!removing) setSourceMenu({ source, x: event.clientX, y: event.clientY });
                }}>
                  <button className="file-name browse-source-row" onClick={() => onOpen(source)}>
                    <Icon size={24} />
                    <span>
                      <strong>{source.name}</strong>
                      <small>{source.kind === 'group' ? '群组' : '分享链接'}</small>
                    </span>
                  </button>
                  <button type="button" className="icon-button browse-source-more"
                    aria-label={`${source.name} 操作菜单`} aria-haspopup={smallScreen ? 'dialog' : 'menu'}
                    aria-expanded={sourceMenu?.source.id === source.id} disabled={removing}
                    onClick={(event) => {
                      const rect = event.currentTarget.getBoundingClientRect();
                      setSourceMenu({ source, x: rect.left, y: rect.bottom });
                    }}>
                    <MoreHorizontal size={20} />
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      )}
      {menu && (
        <FileMenuDialog title="新建" closeLabel="关闭新建菜单" onClose={() => setMenu(false)}>
          <div className="browse-source-menu">{addActions(true)}</div>
        </FileMenuDialog>
      )}
      {sourceMenu && (
        <FileContextMenu x={sourceMenu.x} y={sourceMenu.y} mobile={smallScreen}
          title="项目操作" target={sourceMenu.source.name} onClose={() => setSourceMenu(null)}
          items={[{ label: '从列表移除', icon: Trash2, danger: true,
            action: () => void removeSource(sourceMenu.source) }]} />
      )}
      {adding && accountId && (
        <AddBrowseSource accountId={accountId} kind={adding} onClose={() => setAdding(null)} onAdded={(source) => {
          setSources((current) => [...current.filter((entry) => entry.id !== source.id), source]);
          setAdding(null);
        }} />
      )}
    </section>
  );
}

function AddBrowseSource({ accountId, kind, onClose, onAdded }: {
  accountId: string;
  kind: BrowseSource['kind'];
  onClose: () => void;
  onAdded: (source: BrowseSource) => void;
}) {
  const titleId = useId();
  const [input, setInput] = useState('');
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const submitting = useRef(false);
  const share = kind === 'share';
  const submit = async () => {
    if (submitting.current || !native) return;
    submitting.current = true;
    setBusy(true);
    setError('');
    try {
      const value = share && code.trim()
        ? `提取码：${code.trim()}\n${input.trim()}`
        : input.trim();
      onAdded(await api.addBrowseSource(accountId, kind, value));
    } catch (error) {
      setError(errorText(error));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };
  return (
    <RepositoryDialog titleId={titleId} busy={busy} onDismiss={onClose}>
      <form className="login-panel account-login-modal" onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}>
        <header className="dialog-heading">
          <h2 id={titleId}>{share ? '添加分享链接' : '添加群组ID'}</h2>
          <button className="icon-button" type="button" aria-label="关闭"
            disabled={busy} onClick={onClose}><X size={20} /></button>
        </header>
        <fieldset disabled={busy}>
          {share ? (
            <>
              <label className="form-field">
                分享链接
                <textarea required rows={4} value={input} spellCheck={false}
                  placeholder="粘贴分享链接或完整分享文案"
                  onChange={(event) => setInput(event.target.value)} />
              </label>
              <label className="form-field">
                提取码（可选）
                <input value={code} autoComplete="off" onChange={(event) => setCode(event.target.value)} />
              </label>
            </>
          ) : (
            <label className="form-field">
              群组 ID
              <input required value={input} autoComplete="off"
                onChange={(event) => setInput(event.target.value)} />
            </label>
          )}
          {error && <p className="inline-error" role="alert">{error}</p>}
          <button className="button primary" type="submit" disabled={!native}>
            {busy && <Loader2 size={16} className="spin" />}
            {busy ? '正在添加…' : '添加'}
          </button>
        </fieldset>
      </form>
    </RepositoryDialog>
  );
}
