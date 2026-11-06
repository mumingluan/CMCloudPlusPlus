import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ArrowUpFromLine,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cloud,
  Copy,
  Download,
  ExternalLink,
  File,
  FileArchive,
  FileImage,
  FileMusic,
  FileText,
  FileVideo,
  Folder,
  FolderInput,
  FolderPlus,
  HardDrive,
  Info,
  Link,
  LayoutGrid,
  List,
  ListFilter,
  Loader2,
  MoreHorizontal,
  Pause,
  PanelLeftClose,
  PanelLeftOpen,
  Play,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  Trash2,
  Upload,
  UserRound,
  X,
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  api,
  errorText,
  native,
  android,
  mobile,
  watchTransfers,
  watchScan,
  watchForeground,
  watchDirectory,
} from './api';
import type { ForegroundStatus } from './api';
import { dateText, fileKind, formatBytes, sortFiles, validName, previewSources } from './utils';
import { applyTheme, readPreference, savePreference } from './appearance';
import type { Theme } from './appearance';
import { AccountView } from './AccountView';
import { FileContextMenu } from './FileContextMenu';
import type { FileMenuItem } from './FileContextMenu';
import { useFileDragSelection } from './useFileDragSelection';
import type { AccountSummary, CloudFile, TransferJob } from './types';
import { scalePreview } from './preview';

type Route = 'files' | 'transfers' | 'shares' | 'settings' | 'account';
type Prompt = { kind: 'mkdir' | 'rename' | 'delete' | 'move' | 'copy'; files: CloudFile[] };
const rootFile = (id = '/'): CloudFile => ({
  id,
  name: '全部文件',
  path: id,
  isDir: true,
  size: 0,
  modifiedAt: '',
});
const labels: Record<string, string> = {
  queued: '等待中',
  running: '传输中',
  hashing: '正在校验',
  paused: '已暂停',
  completed: '已完成',
  failed: '传输失败',
  cancelled: '已取消',
  downloading: '下载中',
  uploading: '上传中',
};

function FileGlyph({ file, size = 22 }: { file: CloudFile; size?: number }) {
  const kind = fileKind(file);
  const Icon = file.isDir
    ? Folder
    : kind === '图片'
      ? FileImage
      : kind === '视频'
        ? FileVideo
        : kind === '音频'
          ? FileMusic
          : kind === '压缩包'
            ? FileArchive
            : ['PDF', 'DOCX', 'TXT', 'MD', 'XLSX'].includes(kind)
              ? FileText
              : File;
  return (
    <span
      className={`file-glyph ${file.isDir ? 'folder' : kind === '图片' ? 'image' : kind === '视频' ? 'video' : kind === '压缩包' ? 'archive' : 'document'}`}
    >
      <Icon size={size} strokeWidth={1.6} />
    </span>
  );
}

function App() {
  const preview = useMemo(
    () =>
      !native && new URLSearchParams(location.search).get('preview') === 'scale'
        ? scalePreview()
        : null,
    [],
  );
  const [collapsed, setCollapsed] = useState(
    () => readPreference('cmcloud.sidebar', 'expanded') === 'collapsed',
  );
  const [theme, setTheme] = useState<Theme>(() => {
    const value = readPreference('cmcloud.theme', 'system');
    return ['system', 'light', 'dark'].includes(value) ? (value as Theme) : 'system';
  });
  useEffect(() => {
    applyTheme(theme);
    savePreference('cmcloud.theme', theme);
    const syncWindow = () => {
      applyTheme(theme);
      if (native && !mobile)
        void getCurrentWindow()
          .setTheme(theme === 'system' ? null : theme)
          .catch(() => {});
    };
    syncWindow();
    if (typeof matchMedia !== 'function') return;
    const system = matchMedia('(prefers-color-scheme: dark)');
    const update = syncWindow;
    system.addEventListener('change', update);
    return () => system.removeEventListener('change', update);
  }, [theme]);
  const [route, setRoute] = useState<Route>('files');
  const [account, setAccount] = useState<AccountSummary | null>(preview?.account ?? null);
  const [trail, setTrail] = useState<CloudFile[]>([rootFile()]);
  const [files, setFiles] = useState<CloudFile[]>(preview?.files ?? []);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState('');
  const [query, setQuery] = useState('');
  const deferredQuery = useDeferredValue(query);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [sort, setSort] = useState({ key: 'name', desc: false });
  const [filter, setFilter] = useState('all');
  const [jobs, setJobs] = useState<TransferJob[]>(preview?.jobs ?? []);
  const [quota, setQuota] = useState<{ used: number; total: number } | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);
  const [view, setView] = useState<'list' | 'grid'>(() =>
    readPreference('cmcloud.fileView', 'list') === 'grid' ? 'grid' : 'list',
  );
  const [smallScreen, setSmallScreen] = useState(
    () => mobile || (typeof matchMedia === 'function' && matchMedia('(max-width: 700px)').matches),
  );
  const compact = !(
    mobile ||
    /Android|iPhone|iPad|iPod/i.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  );
  const [gridColumns, setGridColumns] = useState(1);
  const [fileMenu, setFileMenu] = useState<{ x: number; y: number; files: CloudFile[] } | null>(
    null,
  );
  const closeFileMenu = useCallback(() => setFileMenu(null), []);
  const [jobFilter, setJobFilter] = useState('active');
  const [jobOffset, setJobOffset] = useState(0);
  const [jobSummary, setJobSummary] = useState<{ status: string; count: number }[]>([]);
  const [scan, setScan] = useState<{ queued: number; path: string } | null>(null);
  const [foreground, setForeground] = useState<ForegroundStatus | null>(null);
  const [previewFile, setPreviewFile] = useState<CloudFile | null>(null);
  const [transferDetail, setTransferDetail] = useState<TransferJob | null>(null);
  const parent = trail[trail.length - 1];
  const directoryKey = `${account?.id}\0${account?.space}\0${account?.cloudId}\0${parent.id}`;
  const currentDirectory = useRef(directoryKey);
  currentDirectory.current = directoryKey;
  const generation = useRef(0);
  const pageLoading = useRef<number | null>(null);
  const removedFiles = useRef(new Set<string>());
  const changedFiles = useRef(new Map<string, CloudFile>());
  const listingLoaded = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const selected = useMemo(() => files.filter((f) => selection.has(f.id)), [files, selection]);
  const visible = useMemo(
    () =>
      sortFiles(
        files.filter(
          (f) =>
            (!deferredQuery ||
              f.name.toLocaleLowerCase().includes(deferredQuery.toLocaleLowerCase())) &&
            (filter === 'all' || (filter === 'folders' && f.isDir) || fileKind(f) === filter),
        ),
        sort.key,
        sort.desc,
      ),
    [files, deferredQuery, filter, sort],
  );
  const columns = view === 'grid' ? gridColumns : 1;
  const rowHeight = view === 'grid' ? 152 : compact ? 48 : 64;
  const virtual = useVirtualizer({
    count: Math.ceil(visible.length / columns),
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 12,
    getItemKey: (i) => visible[i * columns].id,
  });
  const dragSelection = useFileDragSelection({
    scrollRef,
    files: visible,
    columns,
    rowHeight,
    selection,
    onSelection: setSelection,
    resetKey: `${directoryKey}/${route}/${view}`,
  });
  useEffect(() => {
    const preventMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener('contextmenu', preventMenu);
    const media = typeof matchMedia === 'function' ? matchMedia('(max-width: 700px)') : null;
    const resize = () => setSmallScreen(mobile || !!media?.matches);
    media?.addEventListener('change', resize);
    return () => {
      document.removeEventListener('contextmenu', preventMenu);
      media?.removeEventListener('change', resize);
    };
  }, []);
  useEffect(() => {
    savePreference('cmcloud.fileView', view);
    virtual.measure();
  }, [view, rowHeight, columns]);
  useEffect(() => {
    const area = scrollRef.current;
    if (!area) return;
    const resize = () =>
      setGridColumns(Math.max(1, Math.floor(area.clientWidth / (smallScreen ? 128 : 152))));
    resize();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', resize);
      return () => window.removeEventListener('resize', resize);
    }
    const observer = new ResizeObserver(resize);
    observer.observe(area);
    return () => observer.disconnect();
  }, [route, account?.id, smallScreen]);
  useEffect(closeFileMenu, [
    directoryKey,
    route,
    view,
    deferredQuery,
    filter,
    sort,
    files,
    closeFileMenu,
  ]);
  const alert = useCallback((text: string, error = false) => setNotice({ text, error }), []);
  useEffect(() => {
    if (!notice) return;
    const id = setTimeout(() => setNotice(null), 7000);
    return () => clearTimeout(id);
  }, [notice]);
  useEffect(() => {
    let disposed = false;
    let cleanup = () => {};
    void watchScan((value) => setScan(value)).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    return () => {
      disposed = true;
      cleanup();
    };
  }, []);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let cleanup = () => {};
    void api
      .foreground()
      .then((value) => {
        if (!disposed) setForeground(value);
      })
      .catch(() => {});
    void watchForeground((value) => setForeground(value)).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    return () => {
      disposed = true;
      cleanup();
    };
  }, []);

  const load = useCallback(
    async (dir: CloudFile, next: string | null = null, append = false, merge = false) => {
      if (pageLoading.current === generation.current) return;
      pageLoading.current = generation.current;
      setLoading(true);
      setLoadError('');
      const requestGeneration = generation.current;
      try {
        const page = await api.list(dir, next);
        if (generation.current !== requestGeneration) return;
        setFiles((previous) => {
          if (generation.current !== requestGeneration) return previous;
          const entries = new Map<string, CloudFile>();
          if (append || (merge && page.nextCursor))
            for (const file of previous) entries.set(file.id, file);
          for (const file of page.files) {
            entries.set(file.id, file);
            const changed = changedFiles.current.get(file.id);
            if (changed?.name === file.name && changed.size === file.size)
              changedFiles.current.delete(file.id);
          }
          for (const file of changedFiles.current.values()) entries.set(file.id, file);
          for (const id of removedFiles.current) entries.delete(id);
          return [...entries.values()];
        });
        if (!merge || !listingLoaded.current) {
          setCursor(page.nextCursor ?? null);
          setHasMore(!!page.nextCursor);
        }
        listingLoaded.current = true;
      } catch (e) {
        if (generation.current === requestGeneration && !merge) setLoadError(errorText(e));
      } finally {
        if (pageLoading.current === requestGeneration) {
          pageLoading.current = null;
          setLoading(false);
        }
      }
    },
    [],
  );
  const refresh = useCallback(
    (merge = false) => {
      if (account && currentDirectory.current === directoryKey) {
        // Invalidate an in-flight page so a pre-mutation response cannot restore old entries.
        generation.current++;
        void load(parent, null, false, merge);
      }
    },
    [account, load, parent, directoryKey],
  );
  useEffect(() => {
    removedFiles.current.clear();
    changedFiles.current.clear();
    listingLoaded.current = false;
  }, [account?.id, account?.space, account?.cloudId, parent.id]);
  useEffect(() => {
    let disposed = false;
    let cleanup = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    let publish: ReturnType<typeof setTimeout> | undefined;
    const pending = new Map<string, CloudFile>();
    const synchronize = () => {
      if (pageLoading.current === generation.current) {
        timer = setTimeout(synchronize, 1500);
        return;
      }
      timer = undefined;
      refresh(true);
    };
    void watchDirectory((change) => {
      if (
        disposed ||
        !account ||
        change.accountId !== account.id ||
        change.space !== account.space ||
        change.cloudId !== account.cloudId ||
        change.parentId !== parent.id
      )
        return;
      if (change.file) {
        changedFiles.current.set(change.file.id, change.file);
        removedFiles.current.delete(change.file.id);
        pending.set(change.file.id, change.file);
        if (publish === undefined)
          publish = setTimeout(() => {
            publish = undefined;
            const updates = new Map(pending);
            pending.clear();
            for (const id of removedFiles.current) updates.delete(id);
            setFiles((previous) => [
              ...updates.values(),
              ...previous.filter((entry) => !updates.has(entry.id)),
            ]);
          }, 150);
      }
      // Fixed window: continuous batches still update, with one list request per window.
      if (timer === undefined) timer = setTimeout(synchronize, 1500);
    }).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    return () => {
      disposed = true;
      cleanup();
      clearTimeout(timer);
      clearTimeout(publish);
    };
  }, [account, parent, refresh]);
  const applyAccount = useCallback((value: AccountSummary) => {
    generation.current++;
    setAccount(value);
    setTrail([rootFile(value.rootId)]);
    setFiles([]);
    setSelection(new Set());
    setRoute(value.space === 'share' ? 'shares' : 'files');
    setHasMore(false);
    setCursor(null);
    void api
      .quota()
      .then((value) => {
        const q = value as Record<string, number>;
        if (q)
          setQuota({
            used: Number(q.used ?? q.usedSpace ?? 0),
            total: Number(q.total ?? q.totalSpace ?? 0),
          });
      })
      .catch(() => setQuota(null));
  }, []);
  useEffect(() => {
    if (!native) return;
    api
      .account()
      .then((value) => {
        if (value) applyAccount(value);
      })
      .catch((e) => alert(errorText(e), true));
  }, [applyAccount, alert]);
  useEffect(() => {
    if (account && !preview) {
      const timer = setTimeout(() => void load(parent), 0);
      return () => clearTimeout(timer);
    }
  }, [account, parent, load, preview]);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let cleanup = () => {};
    const fetchJobs = () => {
      void api
        .summary()
        .then(setJobSummary)
        .catch(() => {});
      return api
        .jobs(200, jobOffset, jobFilter)
        .then((incoming) =>
          setJobs((previous) =>
            incoming.map((job) =>
              job.status === 'running'
                ? { ...job, speed: previous.find((p) => p.id === job.id)?.speed ?? 0 }
                : job,
            ),
          ),
        )
        .catch(() => {});
    };
    void fetchJobs();
    const timer = setInterval(fetchJobs, 5000);
    watchTransfers(
      (progress) =>
        setJobs((previous) =>
          previous.map((job) =>
            job.id === progress.id
              ? {
                  ...job,
                  completed: progress.completedBytes,
                  size: progress.totalBytes,
                  speed: progress.speed,
                  status:
                    progress.phase === 'hashing'
                      ? 'hashing'
                      : progress.phase === 'completed'
                        ? 'completed'
                        : 'running',
                }
              : job,
          ),
        ),
      () => void fetchJobs(),
    ).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    return () => {
      disposed = true;
      cleanup();
      clearInterval(timer);
    };
  }, [jobOffset, jobFilter]);
  const openFolder = (file: CloudFile) => {
    generation.current++;
    setFiles([]);
    setSelection(new Set());
    setHasMore(false);
    setTrail([...trail, file]);
  };
  const goTo = (index: number) => {
    generation.current++;
    setFiles([]);
    setSelection(new Set());
    setHasMore(false);
    setTrail(trail.slice(0, index + 1));
  };
  const toggle = (file: CloudFile) =>
    setSelection((previous) => {
      const next = new Set(previous);
      if (next.has(file.id)) next.delete(file.id);
      else next.add(file.id);
      return next;
    });
  const run = async (action: () => Promise<unknown>, success?: string) => {
    setBusy(true);
    try {
      await action();
      if (success) alert(success);
    } catch (e) {
      alert(errorText(e), true);
    } finally {
      setBusy(false);
    }
  };
  const download = (targets: CloudFile[]) =>
    void run(async () => {
      setScan({ queued: 0, path: '正在准备下载' });
      try {
        const results = await api.download(targets);
        if (results.length) {
          setJobs((prev) => [...results, ...prev].slice(0, 200));
          alert('下载任务已加入传输队列');
        }
      } finally {
        setScan(null);
      }
    });
  const upload = () =>
    void run(async () => {
      const results = await api.upload(parent);
      if (results.length) {
        setJobs((prev) => [...results, ...prev].slice(0, 200));
        alert(`${results.length} 个文件已加入上传队列`);
      }
    });
  const uploadFolder = () =>
    void run(async () => {
      setScan({ queued: 0, path: '选择并扫描文件夹' });
      try {
        const result = await api.uploadDirectory(parent);
        if (result.queued)
          alert(
            `${result.queued.toLocaleString()} 个文件已加入上传队列${result.skipped ? `，跳过 ${result.skipped} 个特殊文件` : ''}`,
          );
      } finally {
        setScan(null);
        refresh(true);
      }
    });
  const showLink = (file: CloudFile) =>
    void run(async () => {
      const link = await api.link(file);
      await navigator.clipboard.writeText(link);
    }, '下载链接已复制；链接有效期由云盘服务决定');
  const activeJobs = jobs.filter((j) =>
    ['running', 'queued', 'hashing', 'downloading', 'uploading'].includes(j.status),
  );
  const activeCount = native
    ? jobSummary
        .filter((j) => ['running', 'queued'].includes(j.status))
        .reduce((count, j) => count + j.count, 0)
    : activeJobs.length;
  const jobTotal = native
    ? jobSummary
        .filter(
          (j) =>
            jobFilter === 'all' ||
            (jobFilter === 'completed'
              ? ['completed', 'cancelled'].includes(j.status)
              : !['completed', 'cancelled'].includes(j.status)),
        )
        .reduce((count, j) => count + j.count, 0)
    : jobs.length;
  const readOnly = account?.space === 'share';
  const openFile = (file: CloudFile) => {
    if (file.isDir) openFolder(file);
    else if (fileKind(file) === '视频') setPreviewFile(file);
    else void run(async () => openUrl(await api.link(file)));
  };
  const openFileMenu = (file: CloudFile, x: number, y: number) => {
    if (busy) return;
    const targets = selection.has(file.id) ? selected : [file];
    if (!selection.has(file.id)) setSelection(new Set([file.id]));
    setFileMenu({ x, y, files: targets });
  };
  const menuItems: FileMenuItem[] = [];
  if (fileMenu) {
    const targets = fileMenu.files;
    const single = targets.length === 1 ? targets[0] : null;
    if (single)
      menuItems.push({
        label: single.isDir
          ? '打开文件夹'
          : fileKind(single) === '视频'
            ? '视频预览'
            : ['图片', '音频', 'PDF'].includes(fileKind(single))
              ? '打开预览'
              : '打开文件',
        icon: single.isDir ? Folder : ExternalLink,
        action: () => openFile(single),
      });
    menuItems.push({ label: '下载', icon: Download, action: () => download(targets) });
    if (single && !single.isDir)
      menuItems.push({
        label: '复制下载链接',
        icon: Link,
        action: () => showLink(single),
      });
    if (!readOnly) {
      if (single)
        menuItems.push({
          label: '重命名',
          icon: FileText,
          action: () => setPrompt({ kind: 'rename', files: targets }),
        });
      menuItems.push(
        {
          label: '移动到',
          icon: FolderInput,
          action: () => setPrompt({ kind: 'move', files: targets }),
        },
        { label: '复制到', icon: Copy, action: () => setPrompt({ kind: 'copy', files: targets }) },
        {
          label: '删除',
          icon: Trash2,
          danger: true,
          action: () => setPrompt({ kind: 'delete', files: targets }),
        },
      );
    }
  }
  const nav: { route: Route; label: string; icon: LucideIcon }[] = [
    { route: 'files', label: '我的文件', icon: Folder },
    { route: 'transfers', label: '传输列表', icon: ArrowDownToLine },
    { route: 'shares', label: '分享链接', icon: Link },
  ];

  return (
    <div className={`app-shell ${collapsed ? 'sidebar-collapsed' : ''}`}>
      <aside className="sidebar">
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setRoute('files');
          }}
        >
          <span className="brand-mark">
            <Cloud size={23} strokeWidth={1.9} />
          </span>
          <span>
            CMCloud<span className="brand-plus">++</span>
          </span>
        </a>
        <div className="nav-group">
          <button
            className="nav-item sidebar-toggle"
            aria-label={collapsed ? '展开侧栏' : '收起侧栏'}
            title={collapsed ? '展开侧栏' : '收起侧栏'}
            aria-expanded={!collapsed}
            onClick={() => {
              setCollapsed(!collapsed);
              savePreference('cmcloud.sidebar', !collapsed ? 'collapsed' : 'expanded');
            }}
          >
            {collapsed ? <PanelLeftOpen size={20} /> : <PanelLeftClose size={20} />}
            <span>收起侧栏</span>
          </button>
          {nav.map(({ route: r, label, icon: Icon }) => (
            <button
              key={r}
              title={label}
              aria-label={label}
              aria-current={route === r ? 'page' : undefined}
              className={`nav-item ${route === r ? 'active' : ''}`}
              onClick={() => setRoute(r)}
            >
              <Icon size={20} />
              <span>{label}</span>
              {r === 'transfers' && activeCount > 0 && <span className="count">{activeCount}</span>}
            </button>
          ))}
        </div>
        <nav className="sidebar-utilities" aria-label="会话与偏好">
          <button
            className={`nav-item ${route === 'account' ? 'active' : ''}`}
            title="会话管理"
            aria-label="会话管理"
            aria-current={route === 'account' ? 'page' : undefined}
            onClick={() => setRoute('account')}
          >
            <UserRound size={20} />
            <span>会话管理</span>
          </button>
          <button
            className={`nav-item ${route === 'settings' ? 'active' : ''}`}
            title="设置"
            aria-label="设置"
            aria-current={route === 'settings' ? 'page' : undefined}
            onClick={() => setRoute('settings')}
          >
            <Settings size={20} />
            <span>设置</span>
          </button>
        </nav>
      </aside>
      <div className="main-shell">
        {route === 'account' ? (
          <AccountView
            account={account}
            onConnected={applyAccount}
            onRemovedCurrent={() => {
              generation.current++;
              setAccount(null);
              setFiles([]);
              setSelection(new Set());
              setQuota(null);
              setTrail([rootFile()]);
            }}
            quota={quota}
            onLogout={() =>
              run(async () => {
                await api.logout();
                setAccount(null);
                setFiles([]);
                setQuota(null);
                setTrail([rootFile()]);
              }, '已退出登录')
            }
            alert={alert}
          />
        ) : route === 'settings' ? (
          <SettingsView
            account={account}
            run={run}
            onAccount={applyAccount}
            theme={theme}
            onTheme={setTheme}
          />
        ) : route === 'transfers' ? (
          <section className="workspace transfer-workspace">
            <div className="page-heading">
              <div>
                <h1>传输列表</h1>
              </div>
              <button
                className="button secondary"
                onClick={() =>
                  void run(async () => {
                    await api.clear();
                    setJobs(await api.jobs());
                  }, '已清理完成记录')
                }
              >
                <Trash2 size={16} />
                清理完成记录
              </button>
            </div>
            <div className="transfer-toolbar">
              <div className="tabs">
                {[
                  ['active', '进行中'],
                  ['completed', '已结束'],
                  ['all', '全部任务'],
                ].map(([value, text]) => (
                  <button
                    key={value}
                    className={jobFilter === value ? 'selected' : ''}
                    onClick={() => {
                      setJobFilter(value);
                      setJobOffset(0);
                    }}
                  >
                    {text}
                    {value === 'active' && activeCount > 0 && <span>{activeCount}</span>}
                  </button>
                ))}
              </div>
              <span className="muted">共 {jobTotal.toLocaleString()} 个任务</span>
            </div>
            <>
              {foreground?.supported && (
                <div
                  className={`foreground-note ${foreground.active ? '' : 'limited'}`}
                  role="status"
                >
                  <ShieldCheck size={16} />
                  <span>
                    {foreground.active
                      ? '前台传输服务运行中'
                      : foreground.reason || '添加任务后启用系统前台传输服务'}
                    {!foreground.notificationPermission &&
                      ' · 可在系统设置中允许通知，以显示传输状态'}
                  </span>
                </div>
              )}
            </>
            <TransferList
              jobs={jobs.filter((j) =>
                jobFilter === 'all' || jobFilter === 'completed'
                  ? jobFilter === 'all' || ['completed', 'cancelled'].includes(j.status)
                  : !['completed', 'cancelled'].includes(j.status),
              )}
              control={(id, action) => void run(() => api.control(id, action))}
              open={(id) => void run(() => api.openCompleted(id))}
              reveal={(id) => void run(() => api.openCompleted(id, true))}
              details={setTransferDetail}
              exportFile={(id) =>
                void run(async () => {
                  alert('请选择保存位置，导出期间请保持应用打开');
                  if (await api.exportCompleted(id)) alert('文件已导出');
                })
              }
            />
            <div className="queue-pagination">
              <span>
                {jobTotal
                  ? `${jobOffset + 1}–${Math.min(jobOffset + jobs.length, jobTotal)} / ${jobTotal.toLocaleString()}`
                  : '0 个任务'}
              </span>
              <button
                className="button quiet"
                disabled={jobOffset === 0}
                onClick={() => setJobOffset(Math.max(0, jobOffset - 200))}
              >
                <ArrowLeft size={15} />
                上一页
              </button>
              <button
                className="button quiet"
                disabled={jobOffset + 200 >= jobTotal}
                onClick={() => setJobOffset(jobOffset + 200)}
              >
                下一页
                <ArrowRight size={15} />
              </button>
            </div>
          </section>
        ) : (
          <section className="workspace files-workspace">
            <div className="page-heading">
              <div>
                <h1>
                  {route === 'shares' ? '分享链接' : '我的文件'}
                  {readOnly && <span className="space-tag">只读</span>}
                </h1>
                {preview && <p>示例预览 · 10,000 个文件</p>}
              </div>
              <div className="heading-actions">
                {route === 'shares' && (
                  <button className="button primary" onClick={() => setRoute('account')}>
                    <Link size={17} />
                    添加分享
                  </button>
                )}
              </div>
            </div>
            {!account || (route === 'shares' && !readOnly) ? (
              <ConnectEmpty
                shares={route === 'shares'}
                native={native}
                onConnect={() => setRoute('account')}
                onImport={() =>
                  void run(
                    async () => applyAccount(await api.importOfficial()),
                    '已复用官方客户端账号',
                  )
                }
              />
            ) : (
              <>
                <div className="file-toolbar">
                  <div className="toolbar-actions">
                    {selection.size > 0 ? (
                      <>
                        <span className="selected-count">已选 {selection.size} 项</span>
                        <button
                          className="button quiet"
                          disabled={busy}
                          onClick={() => download(selected)}
                        >
                          <Download size={16} />
                          <span>下载</span>
                        </button>
                        {!readOnly && (
                          <>
                            <button
                              className="button quiet"
                              onClick={() => setPrompt({ kind: 'move', files: selected })}
                            >
                              <FolderInput size={16} />
                              <span>移动</span>
                            </button>
                            <button
                              className="button quiet"
                              onClick={() => setPrompt({ kind: 'copy', files: selected })}
                            >
                              <Copy size={16} />
                              <span>复制</span>
                            </button>
                            <button
                              className="button quiet danger"
                              onClick={() => setPrompt({ kind: 'delete', files: selected })}
                            >
                              <Trash2 size={16} />
                              <span>删除</span>
                            </button>
                          </>
                        )}
                        <button
                          className="icon-button"
                          aria-label="取消选择"
                          onClick={() => setSelection(new Set())}
                        >
                          <X size={16} />
                        </button>
                      </>
                    ) : (
                      <>
                        {!readOnly && (
                          <button className="button primary" disabled={busy} onClick={upload}>
                            <Upload size={17} />
                            上传文件
                          </button>
                        )}
                        {!readOnly && (
                          <button
                            className="button secondary"
                            disabled={busy}
                            onClick={() => setPrompt({ kind: 'mkdir', files: [] })}
                          >
                            <FolderPlus size={17} />
                            新建文件夹
                          </button>
                        )}
                        {!readOnly && (
                          <button className="button quiet" disabled={busy} onClick={uploadFolder}>
                            <FolderInput size={16} />
                            上传文件夹
                          </button>
                        )}
                        <button
                          className="icon-button"
                          disabled={loading}
                          aria-label="刷新目录"
                          onClick={() => refresh()}
                        >
                          <RefreshCw size={17} className={loading ? 'spin' : ''} />
                        </button>
                      </>
                    )}
                  </div>
                  <label className="search">
                    <Search size={17} />
                    <input
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                      placeholder="搜索已加载的文件"
                      aria-label="搜索当前已加载文件"
                    />
                    {query && (
                      <button
                        className="icon-button"
                        aria-label="清除搜索"
                        onClick={() => setQuery('')}
                      >
                        <X size={14} />
                      </button>
                    )}
                  </label>
                </div>
                <>
                  {scan && (
                    <div className="scan-status" role="status">
                      <Loader2 size={16} className="spin" />
                      <span>
                        {scan.path} · 已加入 {scan.queued.toLocaleString()} 项
                      </span>
                      <button className="button quiet" onClick={() => void api.cancelScan()}>
                        取消扫描
                      </button>
                    </div>
                  )}
                </>
                <div className="breadcrumb-bar">
                  <div className="breadcrumbs">
                    {trail.length > 1 && (
                      <button
                        className="icon-button"
                        aria-label="返回上级目录"
                        onClick={() => goTo(trail.length - 2)}
                      >
                        <ArrowLeft size={16} />
                      </button>
                    )}
                    {trail.map((dir, i) => (
                      <span key={`${dir.id}-${i}`}>
                        <button onClick={() => goTo(i)} disabled={i === trail.length - 1}>
                          {i === 0 ? (
                            <>
                              <Cloud size={16} />
                              全部文件
                            </>
                          ) : (
                            dir.name
                          )}
                        </button>
                        {i < trail.length - 1 && <ChevronRight size={14} />}
                      </span>
                    ))}
                  </div>
                  <div className="view-options">
                    <label>
                      <ListFilter size={15} />
                      <select
                        value={filter}
                        onChange={(e) => setFilter(e.target.value)}
                        aria-label="文件类型"
                      >
                        <option value="all">全部类型</option>
                        <option value="folders">文件夹</option>
                        <option value="图片">图片</option>
                        <option value="视频">视频</option>
                        <option value="音频">音频</option>
                        <option value="PDF">PDF</option>
                      </select>
                    </label>
                    <button
                      className="icon-button"
                      aria-label={view === 'list' ? '切换图标模式' : '切换列表模式'}
                      title={view === 'list' ? '切换图标模式' : '切换列表模式'}
                      aria-pressed={view === 'grid'}
                      onClick={() => {
                        setView(view === 'list' ? 'grid' : 'list');
                      }}
                    >
                      {view === 'list' ? <LayoutGrid size={17} /> : <List size={17} />}
                    </button>
                  </div>
                </div>
                <div className="files-area">
                  <div className={`file-table ${view === 'grid' ? 'icon-view' : 'list-view'}`}>
                    <div className="table-head">
                      <label className="checkbox-wrap">
                        <input
                          type="checkbox"
                          aria-label="选择所有已加载文件"
                          checked={visible.length > 0 && visible.every((f) => selection.has(f.id))}
                          onChange={(e) =>
                            setSelection(
                              e.target.checked ? new Set(visible.map((f) => f.id)) : new Set(),
                            )
                          }
                        />
                      </label>
                      {[
                        ['name', '文件名称'],
                        ['size', '大小'],
                        ['modifiedAt', '修改时间'],
                      ].map(([key, label]) => (
                        <button
                          key={key}
                          className={`col-${key}`}
                          onClick={() =>
                            setSort({ key, desc: sort.key === key ? !sort.desc : false })
                          }
                        >
                          {label}
                          {sort.key === key &&
                            (sort.desc ? <ArrowDown size={13} /> : <ArrowUp size={13} />)}
                        </button>
                      ))}
                      <span className="col-menu" />
                    </div>
                    <div className="file-scroll" ref={scrollRef} {...dragSelection.handlers}>
                      {loadError ? (
                        <div className="load-error">
                          <Info size={24} />
                          <h2>目录暂时无法加载</h2>
                          <p>{loadError}</p>
                          <button className="button secondary" onClick={() => refresh()}>
                            <RefreshCw size={16} />
                            重新加载
                          </button>
                        </div>
                      ) : loading && files.length === 0 ? (
                        <div className="skeleton-list" aria-label="正在加载文件">
                          {Array.from({ length: 7 }, (_, i) => (
                            <div key={i}>
                              <span />
                              <span />
                              <span />
                            </div>
                          ))}
                        </div>
                      ) : visible.length === 0 ? (
                        <div className="empty-folder">
                          <Folder size={47} strokeWidth={1.2} />
                          <h2>{query ? '没有匹配的文件' : '这个文件夹还是空的'}</h2>
                          <p>
                            {query
                              ? '换个关键词，或加载更多文件后搜索。'
                              : readOnly
                                ? '此分享中暂无可显示的文件。'
                                : '上传一些文件，或创建文件夹开始整理。'}
                          </p>
                          {!query && !readOnly && (
                            <button className="button secondary" onClick={upload}>
                              <Upload size={16} />
                              上传文件
                            </button>
                          )}
                        </div>
                      ) : (
                        <div className="virtual-body" style={{ height: virtual.getTotalSize() }}>
                          {virtual.getVirtualItems().flatMap((row) =>
                            visible
                              .slice(row.index * columns, (row.index + 1) * columns)
                              .map((file, column) => {
                                return (
                                  <div
                                    key={file.id}
                                    tabIndex={0}
                                    aria-label={file.name}
                                    className={`file-row ${selection.has(file.id) ? 'selected' : ''} ${compact ? 'compact' : ''} ${view === 'grid' ? 'file-card' : ''}`}
                                    style={{
                                      height: rowHeight,
                                      transform: `translateY(${row.start}px)`,
                                      ...(view === 'grid'
                                        ? {
                                            left: `${(column * 100) / columns}%`,
                                            width: `${100 / columns}%`,
                                            right: 'auto',
                                          }
                                        : {}),
                                    }}
                                    onContextMenu={(event) => {
                                      event.preventDefault();
                                      event.currentTarget.focus();
                                      openFileMenu(file, event.clientX, event.clientY);
                                    }}
                                    onKeyDown={(event) => {
                                      if (
                                        event.key === 'ContextMenu' ||
                                        (event.shiftKey && event.key === 'F10')
                                      ) {
                                        event.preventDefault();
                                        const bounds = event.currentTarget.getBoundingClientRect();
                                        openFileMenu(file, bounds.left + 24, bounds.top + 24);
                                      } else if (
                                        event.target === event.currentTarget &&
                                        event.key === ' '
                                      ) {
                                        event.preventDefault();
                                        toggle(file);
                                      } else if (
                                        event.target === event.currentTarget &&
                                        event.key === 'Enter'
                                      ) {
                                        openFile(file);
                                      }
                                    }}
                                    onDoubleClick={(event) => {
                                      if (
                                        !(event.target as HTMLElement).closest(
                                          '.checkbox-wrap, .row-more',
                                        )
                                      )
                                        openFile(file);
                                    }}
                                  >
                                    <label className="checkbox-wrap">
                                      <input
                                        type="checkbox"
                                        aria-label={`选择 ${file.name}`}
                                        checked={selection.has(file.id)}
                                        onChange={() => toggle(file)}
                                      />
                                    </label>
                                    <button
                                      className="file-name"
                                      title={file.name}
                                      onClick={(event) => {
                                        if (
                                          file.isDir &&
                                          !event.ctrlKey &&
                                          !event.metaKey &&
                                          !event.shiftKey
                                        )
                                          openFolder(file);
                                        else if (event.ctrlKey || event.metaKey || event.shiftKey)
                                          toggle(file);
                                        else setSelection(new Set([file.id]));
                                      }}
                                    >
                                      {view === 'grid' && file.thumbnail ? (
                                        <span className="file-artwork">
                                          <img
                                            className="file-thumbnail"
                                            src={file.thumbnail}
                                            alt=""
                                            loading="lazy"
                                            draggable={false}
                                            onError={(event) => {
                                              event.currentTarget.hidden = true;
                                            }}
                                          />
                                          <FileGlyph file={file} size={42} />
                                        </span>
                                      ) : (
                                        <FileGlyph file={file} size={view === 'grid' ? 42 : 22} />
                                      )}
                                      <span>
                                        <strong>{file.name}</strong>
                                        <small>{fileKind(file)}</small>
                                      </span>
                                    </button>
                                    <span className="col-size">
                                      {file.isDir ? '—' : formatBytes(file.size)}
                                    </span>
                                    <span className="col-modifiedAt">
                                      {dateText(file.modifiedAt)}
                                    </span>
                                    <button
                                      className="icon-button row-more"
                                      aria-label={`${file.name} 操作菜单`}
                                      aria-haspopup="menu"
                                      disabled={busy}
                                      onClick={(event) => {
                                        const bounds = event.currentTarget.getBoundingClientRect();
                                        openFileMenu(file, bounds.right, bounds.bottom);
                                      }}
                                    >
                                      <MoreHorizontal size={19} />
                                    </button>
                                  </div>
                                );
                              }),
                          )}
                          {dragSelection.box && (
                            <div className="file-selection-box" style={dragSelection.box} />
                          )}
                        </div>
                      )}
                      {hasMore && (
                        <div className="load-more">
                          <button
                            className="button quiet"
                            disabled={loading}
                            onClick={() => void load(parent, cursor, true)}
                          >
                            {loading ? (
                              <Loader2 size={16} className="spin" />
                            ) : (
                              <ChevronDown size={16} />
                            )}
                            加载更多文件
                          </button>
                        </div>
                      )}
                    </div>
                    <footer className="file-footer">
                      <span>
                        已加载 {files.length.toLocaleString()} 项{hasMore ? ' · 还有更多文件' : ''}
                        {query && ` · ${visible.length} 项匹配`}
                      </span>
                      <span>
                        {selection.size
                          ? `已选 ${selection.size} 项 · ${formatBytes(selected.reduce((sum, f) => sum + f.size, 0))}`
                          : !compact
                            ? '从复选框滑动多选'
                            : '拖动框选 · Ctrl 追加选择'}
                      </span>
                    </footer>
                  </div>
                </div>
              </>
            )}
          </section>
        )}
      </div>

      {fileMenu && (
        <FileContextMenu x={fileMenu.x} y={fileMenu.y} items={menuItems} onClose={closeFileMenu} />
      )}
      {notice && (
        <div
          className={`toast ${notice.error ? 'error' : ''}`}
          role={notice.error ? 'alert' : 'status'}
        >
          {notice.error ? <Info size={19} /> : <CheckCircle2 size={19} />}
          <span>{notice.text}</span>
          <button className="icon-button" aria-label="关闭提示" onClick={() => setNotice(null)}>
            <X size={16} />
          </button>
        </div>
      )}
      {transferDetail && (
        <TransferDetails
          job={jobs.find((job) => job.id === transferDetail.id) ?? transferDetail}
          onClose={() => setTransferDetail(null)}
          control={(id, action) => void run(() => api.control(id, action))}
        />
      )}
      {previewFile && (
        <VideoPreview file={previewFile} onClose={() => setPreviewFile(null)} alert={alert} />
      )}
      {prompt && (
        <OperationDialog
          prompt={prompt}
          parent={parent}
          root={trail[0]}
          onClose={() => setPrompt(null)}
          onRemoved={(ids) => {
            for (const id of ids) {
              removedFiles.current.add(id);
              changedFiles.current.delete(id);
            }
            setFiles((previous) => previous.filter((file) => !removedFiles.current.has(file.id)));
            setSelection(
              (previous) => new Set([...previous].filter((id) => !removedFiles.current.has(id))),
            );
            refresh(true);
          }}
          onDone={(name) => {
            if (prompt.kind === 'move') {
              for (const file of prompt.files) {
                removedFiles.current.add(file.id);
                changedFiles.current.delete(file.id);
              }
              setFiles((previous) => previous.filter((file) => !removedFiles.current.has(file.id)));
            } else if (prompt.kind === 'rename' && name) {
              const file = { ...prompt.files[0], name };
              changedFiles.current.set(file.id, file);
              setFiles((previous) =>
                previous.map((entry) => (entry.id === file.id ? file : entry)),
              );
            }
            setPrompt(null);
            setSelection(new Set());
            if (prompt.kind !== 'delete') refresh(true);
          }}
          alert={alert}
        />
      )}
    </div>
  );
}

function ConnectEmpty({
  shares,
  native,
  onConnect,
  onImport,
}: {
  shares: boolean;
  native: boolean;
  onConnect: () => void;
  onImport: () => void;
}) {
  return (
    <div className="connect-empty">
      <div className="connect-content">
        <div className="connect-visual">
          <Cloud size={64} strokeWidth={1.1} />
          <span>
            <Check size={16} />
          </span>
        </div>
        <h2>{shares ? '分享里的文件，也能井井有条。' : '让文件各归其位。'}</h2>
        <p>
          {shares
            ? '添加移动云盘分享链接，支持提取码和多个分享合并浏览。'
            : '连接移动云盘，浏览个人与群组空间，轻松整理文件，让大文件传输有迹可循。'}
        </p>
        <div className="connect-actions">
          <button className="button primary" onClick={onConnect}>
            {shares ? <Link size={17} /> : <Cloud size={18} />}{' '}
            {shares ? '添加分享链接' : '连接移动云盘'}
            <ArrowRight size={17} />
          </button>
          {!shares && !mobile && (
            <button className="button secondary" disabled={!native} onClick={onImport}>
              <HardDrive size={17} />
              复用官方客户端账号
            </button>
          )}
        </div>
        {!native && (
          <span className="preview-note">当前为界面预览 · 登录与传输请在 Tauri 应用中使用</span>
        )}
      </div>
      <div className="connect-notes">
        <span>
          <Folder size={18} />
          <strong>不止一种空间</strong>
          <small>个人、群组与分享</small>
        </span>
        <span>
          <ArrowDownToLine size={18} />
          <strong>传输进度随时可见</strong>
          <small>暂停、继续与失败重试</small>
        </span>
        <span>
          <ShieldCheck size={18} />
          <strong>凭据留在你的设备</strong>
          <small>本机保存，通过官方接口访问</small>
        </span>
      </div>
    </div>
  );
}

function TransferList({
  jobs,
  control,
  open,
  reveal,
  exportFile,
  details,
}: {
  jobs: TransferJob[];
  control: (id: string, action: string) => void;
  open: (id: string) => void;
  reveal: (id: string) => void;
  exportFile: (id: string) => void;
  details: (job: TransferJob) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: jobs.length,
    getScrollElement: () => ref.current,
    estimateSize: () => 120,
    overscan: 8,
  });
  if (!jobs.length)
    return (
      <div className="transfer-empty">
        <ArrowDownToLine size={45} strokeWidth={1.2} />
        <h2>队列很轻，等你的下一份文件。</h2>
        <p>在文件列表中上传或下载，任务会显示在这里。</p>
        <div>
          <ShieldCheck size={16} />
          任务进度在本机保存，失败后可以重试。
        </div>
      </div>
    );
  return (
    <div className="transfer-scroll" ref={ref}>
      <div style={{ height: virtual.getTotalSize(), position: 'relative' }}>
        {virtual.getVirtualItems().map((item) => {
          const job = jobs[item.index];
          const progress = job.size > 0 ? Math.min((job.completed / job.size) * 100, 100) : 0;
          const active = ['running', 'hashing', 'downloading', 'uploading', 'queued'].includes(
            job.status,
          );
          return (
            <article
              className={`transfer-row ${job.status}`}
              key={job.id}
              style={{ height: item.size, transform: `translateY(${item.start}px)` }}
            >
              <span className="transfer-direction">
                {job.direction === 'upload' ? (
                  <ArrowUpFromLine size={23} />
                ) : (
                  <ArrowDownToLine size={23} />
                )}
              </span>
              <div className="transfer-info">
                <div className="transfer-title">
                  <button
                    className="transfer-name"
                    onClick={() => details(job)}
                    aria-label={`查看 ${job.name} 传输详情`}
                  >
                    {job.name}
                  </button>
                  <span>{labels[job.status] || job.status}</span>
                </div>
                <progress value={progress} max={100} />
                <div className="transfer-meta">
                  <span>
                    {formatBytes(job.completed)} / {formatBytes(job.size)}
                    {job.speed > 0 && ` · ${formatBytes(job.speed)}/s`}
                  </span>
                  <span>
                    {job.status === 'hashing'
                      ? '分块读取，计算文件校验值'
                      : job.error || job.localPath}
                  </span>
                </div>
              </div>
              <div className="transfer-controls">
                {active ? (
                  <button
                    className="icon-button"
                    aria-label={`暂停 ${job.name}`}
                    onClick={() => control(job.id, 'pause')}
                  >
                    <Pause size={17} />
                  </button>
                ) : ['paused', 'failed', 'cancelled'].includes(job.status) ? (
                  <button
                    className="icon-button"
                    aria-label={`继续 ${job.name}`}
                    onClick={() => control(job.id, job.status === 'paused' ? 'resume' : 'retry')}
                  >
                    <Play size={17} />
                  </button>
                ) : job.direction === 'download' && job.status === 'completed' ? (
                  <>
                    <button
                      className="icon-button"
                      aria-label={`打开 ${job.name}`}
                      onClick={() => open(job.id)}
                    >
                      <ExternalLink size={17} />
                    </button>
                    {android && (
                      <button
                        className="icon-button"
                        aria-label={`导出 ${job.name}`}
                        onClick={() => exportFile(job.id)}
                      >
                        <Download size={17} />
                      </button>
                    )}
                    <button
                      className="icon-button reveal-download"
                      aria-label={`在文件夹中显示 ${job.name}`}
                      onClick={() => reveal(job.id)}
                    >
                      <Folder size={17} />
                    </button>
                  </>
                ) : (
                  <CheckCircle2 size={18} />
                )}
                {job.status !== 'completed' && job.status !== 'cancelled' && (
                  <button
                    className="icon-button"
                    aria-label={`取消 ${job.name}`}
                    onClick={() => control(job.id, 'cancel')}
                  >
                    <X size={17} />
                  </button>
                )}
              </div>
            </article>
          );
        })}
      </div>
    </div>
  );
}

function TransferDetails({
  job,
  onClose,
  control,
}: {
  job: TransferJob;
  onClose: () => void;
  control: (id: string, action: string) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    dialog.current?.showModal();
    return () => dialog.current?.close();
  }, []);
  return (
    <dialog
      className="operation-dialog transfer-dialog"
      ref={dialog}
      aria-labelledby="transfer-detail-title"
      onCancel={onClose}
    >
      <div className="dialog-heading">
        <h2 id="transfer-detail-title">传输详情</h2>
        <button className="icon-button" aria-label="关闭传输详情" onClick={onClose}>
          <X size={19} />
        </button>
      </div>
      <h3>{job.name}</h3>
      <dl>
        <dt>状态</dt>
        <dd>{labels[job.status] || job.status}</dd>
        <dt>进度</dt>
        <dd>
          {formatBytes(job.completed)} / {formatBytes(job.size)}
        </dd>
        <dt>本地路径</dt>
        <dd>{job.localPath}</dd>
      </dl>
      {job.error && (
        <div className="transfer-full-error" role="alert">
          <strong>失败原因</strong>
          <p>{job.error}</p>
        </div>
      )}
      <div className="dialog-actions">
        <button className="button secondary" onClick={onClose}>
          关闭
        </button>
        {['failed', 'paused', 'cancelled'].includes(job.status) && (
          <button
            className="button primary"
            disabled={!native}
            onClick={() => {
              control(job.id, job.status === 'paused' ? 'resume' : 'retry');
              onClose();
            }}
          >
            <Play size={16} />
            {job.status === 'paused' ? '继续传输' : '重试任务'}
          </button>
        )}
      </div>
    </dialog>
  );
}

function VideoPreview({
  file,
  onClose,
  alert,
}: {
  file: CloudFile;
  onClose: () => void;
  alert: (message: string, error?: boolean) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [sources, setSources] = useState<{ url: string; label: string }[]>([]);
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    dialog.current?.showModal();
    let disposed = false;
    void (async () => {
      try {
        let values = previewSources(await api.preview(file));
        if (!values.length) values = [{ url: await api.link(file), label: '原始文件' }];
        if (!disposed) {
          setSources(values);
          setSource(values[0].url);
        }
      } catch (e) {
        if (!disposed) setError(errorText(e));
      } finally {
        if (!disposed) setLoading(false);
      }
    })();
    return () => {
      disposed = true;
      dialog.current?.close();
    };
  }, [file]);
  return (
    <dialog ref={dialog} className="operation-dialog video-dialog" onCancel={onClose}>
      <div className="dialog-heading">
        <h2>{file.name}</h2>
        <button className="icon-button" aria-label="关闭视频预览" onClick={onClose}>
          <X size={19} />
        </button>
      </div>
      {loading ? (
        <p className="preview-loading">
          <Loader2 size={18} className="spin" />
          正在获取播放源…
        </p>
      ) : error ? (
        <div className="inline-error" role="alert">
          <Info size={17} />
          {error}
        </div>
      ) : (
        <>
          <video
            key={source}
            src={source}
            controls
            playsInline
            preload="metadata"
            onError={() =>
              setError('当前播放源无法在设备内播放。可打开外部预览，或下载后使用本机播放器。')
            }
          />
          {sources.length > 1 && (
            <label className="form-field">
              播放源
              <select
                value={source}
                onChange={(e) => {
                  setSource(e.target.value);
                  setError('');
                }}
              >
                {sources.map((item) => (
                  <option key={item.url} value={item.url}>
                    {item.label}
                  </option>
                ))}
              </select>
            </label>
          )}
        </>
      )}
      {source && (
        <div className="dialog-actions">
          <button
            className="button secondary"
            onClick={() => void openUrl(source).catch((e) => alert(errorText(e), true))}
          >
            <ExternalLink size={16} />
            外部预览
          </button>
          <button
            className="button secondary"
            onClick={() =>
              void navigator.clipboard
                .writeText(source)
                .then(() => alert('播放链接已复制'))
                .catch((e) => alert(errorText(e), true))
            }
          >
            <Copy size={16} />
            复制播放链接
          </button>
        </div>
      )}
    </dialog>
  );
}

function SettingsView({
  account,
  run,
  onAccount,
  theme,
  onTheme,
}: {
  account: AccountSummary | null;
  run: (action: () => Promise<unknown>, success?: string) => Promise<void>;
  onAccount: (account: AccountSummary) => void;
  theme: Theme;
  onTheme: (theme: Theme) => void;
}) {
  const [concurrency, setConcurrency] = useState(4);
  const [saved, setSaved] = useState(false);
  const [partSize, setPartSize] = useState(0);
  const [large, setLarge] = useState(false);
  const [real, setReal] = useState(true);
  useEffect(() => {
    if (!native) return;
    void api
      .options()
      .then((options) => {
        setConcurrency(options.concurrency);
        setPartSize(options.customUploadPartSize);
        setReal(options.reportRealSize);
        setLarge(options.useLargeThumbnail);
        setSaved(true);
      })
      .catch(() => {});
  }, []);
  return (
    <section className="workspace settings-workspace">
      <div className="page-heading">
        <div>
          <h1>设置</h1>
        </div>
      </div>
      <div className="settings-panel">
        <h2>外观</h2>
        <div className="setting-row">
          <div>
            <h3>主题</h3>
            <p>跟随系统，或选择喜欢的明暗外观。</p>
          </div>
          <select
            aria-label="主题"
            value={theme}
            onChange={(e) => onTheme(e.target.value as Theme)}
          >
            <option value="system">跟随系统</option>
            <option value="light">浅色</option>
            <option value="dark">深色</option>
          </select>
        </div>
        <h2>传输</h2>
        <div className="setting-row">
          <div>
            <h3>同时传输任务</h3>
            <p>控制并发，兼顾带宽与服务端限流。</p>
          </div>
          <select
            value={concurrency}
            onChange={(e) => setConcurrency(Number(e.target.value))}
            aria-label="同时传输任务数"
          >
            {[1, 2, 3, 4, 6, 8].map((n) => (
              <option key={n} value={n}>
                {n} 个任务
              </option>
            ))}
          </select>
        </div>
        <div className="setting-row">
          <div>
            <h3>上传分片大小</h3>
            <p>自动选择 100 / 512 MB，TB 文件不会整体载入内存。</p>
          </div>
          <select
            aria-label="上传分片大小"
            value={partSize}
            onChange={(e) => setPartSize(Number(e.target.value))}
          >
            {[
              [0, '自动'],
              [32, '32 MB'],
              [100, '100 MB'],
              [256, '256 MB'],
              [512, '512 MB'],
            ].map(([n, label]) => (
              <option key={n} value={Number(n) * 1024 * 1024}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <Toggle
          title="上报真实文件大小"
          description="按实际文件大小创建上传任务，与驱动默认行为一致。"
          value={real}
          onChange={setReal}
        />
        <h2>文件浏览</h2>
        <Toggle
          title="使用大尺寸缩略图"
          description="优先加载更清晰的图片预览，会增加网络用量。"
          value={large}
          onChange={setLarge}
        />
        <div className="settings-save">
          <button
            className="button primary"
            disabled={!native || !saved}
            onClick={() =>
              void run(async () => {
                await api.settings(concurrency);
                if (account)
                  onAccount(
                    await api.updateOptions({
                      customUploadPartSize: partSize,
                      reportRealSize: real,
                      useLargeThumbnail: large,
                    }),
                  );
              }, '设置已保存')
            }
          >
            <Check size={17} />
            保存设置
          </button>
        </div>
      </div>
      <div className="settings-about">
        <Cloud size={25} />
        <div>
          <strong>CMCloud++</strong>
          <p>Tauri 2 · 第三方中国移动云盘客户端 · v0.1.6</p>
          <small>支持 Windows 与 Android，保留 iOS 工程兼容性。</small>
          <p>
            <a
              href="https://github.com/mumingluan/CMCloudPlusPlus"
              target="_blank"
              rel="noopener noreferrer"
              onClick={(event) => {
                if (native) {
                  event.preventDefault();
                  void run(async () => openUrl('https://github.com/mumingluan/CMCloudPlusPlus'));
                }
              }}
            >
              GitHub 源码
            </a>
          </p>
        </div>
      </div>
    </section>
  );
}
function Toggle({
  title,
  description,
  value,
  onChange,
}: {
  title: string;
  description: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="setting-row">
      <div>
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
      <button
        className={`toggle ${value ? 'on' : ''}`}
        role="switch"
        aria-checked={value}
        aria-label={title}
        onClick={() => onChange(!value)}
      >
        <span />
      </button>
    </div>
  );
}

export function OperationDialog({
  prompt,
  parent,
  root,
  onClose,
  onDone,
  onRemoved,
  alert,
}: {
  prompt: Prompt;
  parent: CloudFile;
  root: CloudFile;
  onClose: () => void;
  onDone: (name?: string) => void;
  onRemoved?: (ids: string[]) => void;
  alert: (text: string, error?: boolean) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(prompt.kind === 'rename' ? (prompt.files[0]?.name ?? '') : '');
  const [deleteTargets, setDeleteTargets] = useState(prompt.files);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [dest, setDest] = useState(root);
  const [dirs, setDirs] = useState<CloudFile[]>([]);
  const [destTrail, setDestTrail] = useState<CloudFile[]>([root]);
  const [dirCursor, setDirCursor] = useState<string | null>(null);
  useEffect(() => {
    dialog.current?.showModal();
    return () => dialog.current?.close();
  }, []);
  const [dirLoading, setDirLoading] = useState(false);
  const destinationRequest = useRef(0);
  const pagePending = useRef(false);
  const submitting = useRef(false);
  useEffect(() => {
    if (!['move', 'copy'].includes(prompt.kind)) return;
    const request = ++destinationRequest.current;
    let disposed = false;
    pagePending.current = true;
    setDirs([]);
    setDirCursor(null);
    setDirLoading(true);
    setError('');
    void api
      .list(dest, null)
      .then((page) => {
        if (!disposed && destinationRequest.current === request) {
          setDirs(page.files.filter((f) => f.isDir));
          setDirCursor(page.nextCursor ?? null);
        }
      })
      .catch((e) => {
        if (!disposed && destinationRequest.current === request) setError(errorText(e));
      })
      .finally(() => {
        if (!disposed && destinationRequest.current === request) {
          pagePending.current = false;
          setDirLoading(false);
        }
      });
    return () => {
      disposed = true;
    };
  }, [dest, prompt.kind]);
  const navigateDestination = (directory: CloudFile, path: CloudFile[]) => {
    destinationRequest.current++;
    pagePending.current = false;
    setDirs([]);
    setDirCursor(null);
    setDirLoading(true);
    setDest(directory);
    setDestTrail(path);
  };
  const loadDestinationPage = async () => {
    if (pagePending.current || !dirCursor) return;
    const request = destinationRequest.current;
    pagePending.current = true;
    setDirLoading(true);
    try {
      const page = await api.list(dest, dirCursor);
      if (destinationRequest.current !== request) return;
      setDirs((previous) => {
        const ids = new Set(previous.map((file) => file.id));
        return [...previous, ...page.files.filter((file) => file.isDir && !ids.has(file.id))];
      });
      if (page.nextCursor === dirCursor) {
        setError('目录分页没有前进，请重新选择目录');
        setDirCursor(null);
      } else setDirCursor(page.nextCursor ?? null);
    } catch (e) {
      if (destinationRequest.current === request) setError(errorText(e));
    } finally {
      if (destinationRequest.current === request) {
        pagePending.current = false;
        setDirLoading(false);
      }
    }
  };
  const confirm = async () => {
    if (submitting.current || busy || dirLoading) return;
    if (['mkdir', 'rename'].includes(prompt.kind) && !validName(name)) {
      setError('请输入有效名称，名称不能包含斜杠。');
      return;
    }
    submitting.current = true;
    setBusy(true);
    setError('');
    try {
      if (prompt.kind === 'mkdir') await api.mkdir(parent, name.trim());
      else if (prompt.kind === 'rename') await api.rename(prompt.files[0], name.trim());
      else if (prompt.kind === 'delete') {
        const result = await api.remove(deleteTargets);
        const removed = new Set(result.removedIds);
        setDeleteTargets((previous) => previous.filter((file) => !removed.has(file.id)));
        onRemoved?.(result.removedIds);
        if (result.errors.length)
          throw new Error(`部分项目删除失败；已成功的操作保留。\n${result.errors.join('\n')}`);
      } else await api.relocate(prompt.files, dest, prompt.kind === 'copy');
      onDone(name.trim());
      alert(prompt.kind === 'delete' ? '所选文件已删除' : '操作已完成');
    } catch (e) {
      setError(errorText(e));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };
  const title = {
    mkdir: '新建文件夹',
    rename: '重命名',
    delete: '删除所选文件',
    move: '移动到',
    copy: '复制到',
  }[prompt.kind];
  return (
    <dialog
      className="operation-dialog"
      ref={dialog}
      onCancel={(e) => {
        if (busy) e.preventDefault();
        else onClose();
      }}
    >
      <div className="dialog-heading">
        <h2>{title}</h2>
        <button className="icon-button" disabled={busy} aria-label="关闭" onClick={onClose}>
          <X size={19} />
        </button>
      </div>
      {prompt.kind === 'delete' ? (
        <div className="delete-content">
          <Trash2 size={30} />
          <p>将删除 {deleteTargets.length} 个项目。</p>
          <small>删除由云盘接口执行。继续前请确认所选文件。</small>
          <ul>
            {deleteTargets.slice(0, 5).map((file) => (
              <li key={file.id}>{file.name}</li>
            ))}
          </ul>
          {prompt.files.length > 5 && <small>以及另外 {prompt.files.length - 5} 项</small>}
        </div>
      ) : ['move', 'copy'].includes(prompt.kind) ? (
        <>
          <div className="destination-path">
            {destTrail.map((d, i) => (
              <button
                key={i}
                disabled={busy || i === destTrail.length - 1}
                onClick={() => navigateDestination(d, destTrail.slice(0, i + 1))}
              >
                {d.name}
                <ChevronRight size={13} />
              </button>
            ))}
          </div>
          <div className="destination-list">
            {dirs
              .filter((d) => !prompt.files.some((f) => f.id === d.id))
              .map((d) => (
                <button
                  key={d.id}
                  disabled={busy}
                  onClick={() => navigateDestination(d, [...destTrail, d])}
                >
                  <Folder size={20} />
                  {d.name}
                  <ChevronRight size={16} />
                </button>
              ))}
            {dirLoading && (
              <p className="muted" role="status">
                <Loader2 size={16} className="spin" />
                正在读取目标目录…
              </p>
            )}
            {!dirLoading && !dirs.length && !error && <p className="muted">当前目录没有子文件夹</p>}
            {dirCursor && (
              <button disabled={dirLoading || busy} onClick={() => void loadDestinationPage()}>
                加载更多文件夹
              </button>
            )}
          </div>
          <p className="muted">
            将 {prompt.files.length} 个项目{prompt.kind === 'copy' ? '复制' : '移动'}到「{dest.name}
            」
          </p>
        </>
      ) : (
        <label className="form-field">
          名称
          <input
            autoFocus
            disabled={busy}
            value={name}
            maxLength={255}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void confirm();
              }
            }}
          />
        </label>
      )}
      {error && (
        <div className="inline-error" role="alert">
          <Info size={17} />
          {error}
        </div>
      )}
      <div className="dialog-actions">
        <button className="button secondary" disabled={busy} onClick={onClose}>
          取消
        </button>
        <button
          className={`button ${prompt.kind === 'delete' ? 'destructive' : 'primary'}`}
          disabled={busy || dirLoading || (prompt.kind === 'delete' && deleteTargets.length === 0)}
          onClick={() => void confirm()}
        >
          {busy && <Loader2 size={16} className="spin" />}
          {busy ? '处理中…' : prompt.kind === 'delete' ? '确认删除' : '确认'}
        </button>
      </div>
    </dialog>
  );
}
export default App;
