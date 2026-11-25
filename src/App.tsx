import { useCallback, useDeferredValue, useEffect, useId, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowDownToLine,
  ArrowLeft,
  ArrowRight,
  ArrowUpFromLine,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Cloud,
  Copy,
  Download,
  ExternalLink,
  FileText,
  Folder,
  FolderInput,
  FolderPlus,
  HardDrive,
  Info,
  Link,
  LogOut,
  ListFilter,
  Loader2,
  Maximize,
  Minimize,
  Pause,
  PanelLeftClose,
  PanelLeftOpen,
  Play,
  Plus,
  RefreshCw,
  Search,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  Upload,
  Users,
  UserRound,
  X,
} from 'lucide-react';
import type { CSSProperties, ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  api,
  normalizeJob,
  errorText,
  native,
  android,
  mobile,
  setVideoImmersive,
  watchTransfers,
  watchScan,
  watchDirectory,
  watchFileDrop,
} from './api';
import {
  fileKind,
  formatBytes,
  formatCloudPath,
  formatRepositoryPath,
  parseAddressPath,
  sortFiles,
  validName,
  previewSources,
  isVendorFolder,
} from './utils';
import { TransferOverview } from './TransferOverview';
import { TransferBatchActions, BatchCancelDialog, batchCounts } from './TransferBatchActions';
import type { BatchAction, BatchDirection } from './TransferBatchActions';
import { isOngoing, matchesTransfer, overviewFromJobs } from './transferView';
import { TransferCategories } from './TransferCategories';
import type { TransferFilter, TransferOverview as Overview } from './transferView';
import { applyTheme, readAppearance, readPreference, savePreference } from './appearance';
import type { Theme } from './appearance';
import { AccountView } from './AccountView';
import { BrowseSources } from './BrowseSources';
import { FileContextMenu } from './FileContextMenu';
import {
  FileBreadcrumbs,
  FileRowContents,
  FileRowMenuButton,
  FileSelectAll,
  FileTableHead,
  fileGridSizes,
  fileViewLabels,
  type AddressBarHandle,
  type FileView,
} from './FileBrowserParts';
import { FileViewOptions } from './FileViewOptions';
import { usePullToRefresh } from './usePullToRefresh';
import { useFileUploadDrop } from './useFileUploadDrop';
import type { FileMenuItem } from './FileContextMenu';
import { useFileDragSelection } from './useFileDragSelection';
import { useMobileFileLongPress } from './useMobileFileLongPress';
import { useRecursiveFileSearch } from './useRecursiveFileSearch';
import type { AccountSummary, BrowseSource, CloudFile, TransferJob } from './types';
import { scalePreview } from './preview';
import { RepositoryWorkspace, RepositoryCreateDialog, AxuanStorMark } from './RepositoryWorkspace';
import type { RepositoryControls } from './RepositoryWorkspace';
import { repositoryFile, repositoryOperations, repositoryTrail } from './repositoryFiles';
import { RepositorySettings } from './RepositorySettings';
import { useRepositoryDiscovery } from './useRepositoryDiscovery';
import { repositoryApi, repositoryPreviewUrl, watchRepositoryDirectory } from './repositoryApi';
import type { RepositoryPreviewStart } from './repositoryApi';
import type { RepositoryProbe, RepositoryView } from './repositoryApi';
import { CryptWorkspace, CryptCreateDialog, OpenListCryptMark } from './CryptWorkspace';
import type { CryptControls } from './CryptWorkspace';
import { cryptFile, cryptOperations, cryptTrail } from './cryptFiles';
import { useCryptDiscovery } from './useCryptDiscovery';
import { cryptApi, watchCryptDirectory } from './cryptApi';
import type { CryptFolderProbe, CryptPreviewStart, CryptView } from './cryptApi';
import { contentKind } from './contentPreviewApi';
import { FileContentPreview } from './FileContentPreview';
import { PreviewWorkspace } from './PreviewWorkspace';
import { PreviewZoom } from './PreviewZoom';

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
const sharesRoot: CloudFile = { ...rootFile('browse-sources'), name: '群组与分享' };
const labels: Record<string, string> = {
  queued: '等待中',
  running: '传输中',
  hashing: '正在校验',
  encrypting: '正在加密',
  decrypting: '正在解密',
  publishing: '正在提交目录',
  paused: '已暂停',
  completed: '已完成',
  failed: '传输失败',
  cancelled: '已取消',
  downloading: '下载中',
  uploading: '上传中',
};


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
  const initialAppearance = useMemo(readAppearance, []);
  const [theme, setTheme] = useState<Theme>(initialAppearance.theme);
  const [oled, setOled] = useState(initialAppearance.oled);
  const [systemDark, setSystemDark] = useState(
    () => typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches,
  );
  useEffect(() => {
    if (typeof matchMedia !== 'function') return;
    const media = matchMedia('(prefers-color-scheme: dark)');
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);
  useEffect(() => {
    applyTheme(theme, oled);
    savePreference('cmcloud.theme', theme);
    savePreference('cmcloud.oledPureBlack', String(oled));
    if (native && !mobile)
      void getCurrentWindow()
        .setTheme(theme === 'system' ? null : theme)
        .catch(() => {});
  }, [theme, oled, systemDark]);
  const [route, setRoute] = useState<Route>('files');
  const [restoring, setRestoring] = useState(native);
  const mainRef = useRef<HTMLDivElement>(null);
  const previousRoute = useRef<Route>(route);
  useEffect(() => {
    const order: Route[] = ['files', 'transfers', 'shares', 'settings'];
    const previous = previousRoute.current;
    previousRoute.current = route;
    const surface = Array.from(mainRef.current?.children ?? []).find(
      (child) => child instanceof HTMLElement && !child.hidden,
    );
    if (previous === route || !surface || typeof surface.animate !== 'function') return;
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const direction = order.indexOf(route) >= order.indexOf(previous) ? 1 : -1;
    const animation = surface.animate(
      [
        { opacity: 0.65, transform: `translateX(${direction * 10}px)` },
        { opacity: 1, transform: 'translateX(0)' },
      ],
      { duration: 180, easing: 'cubic-bezier(.16,1,.3,1)' },
    );
    return () => animation.cancel();
  }, [route]);
  const [account, setAccount] = useState<AccountSummary | null>(preview?.account ?? null);
  useEffect(() => {
    const phone = account?.phone || account?.account;
    const title = phone ? `${phone} - CMCloud++` : 'CMCloud++';
    document.title = title;
    if (native && !mobile)
      void getCurrentWindow()
        .setTitle(title)
        .catch(() => {});
  }, [account?.phone, account?.account]);
  const [personalTrail, setPersonalTrail] = useState<CloudFile[]>([rootFile()]);
  const [shareTrail, setShareTrail] = useState<CloudFile[]>([sharesRoot]);
  const trail = route === 'shares' ? shareTrail : personalTrail;
  const setTrail = route === 'shares' ? setShareTrail : setPersonalTrail;
  const [repositoryFolder, setRepositoryFolder] = useState<{
    file: CloudFile;
    probe?: RepositoryProbe;
    creating?: boolean;
    initialView?: RepositoryView;
    initialDirectory?: string;
    cloudPath?: string;
  } | null>(null);
  const [managedRepository, setManagedRepository] = useState<{
    file: CloudFile;
    probe: RepositoryProbe;
  } | null>(null);
  const [creatingRepository, setCreatingRepository] = useState(false);
  const [repositoryEpoch, setRepositoryEpoch] = useState(0);
  const [repositoryView, setRepositoryView] = useState<RepositoryView | null>(null);
  const [repositoryBusy, setRepositoryBusy] = useState(false);
  const repositoryControls = useRef<RepositoryControls>(null);
  const repositoryManageFocus = useRef<HTMLButtonElement>(null);
  const repositoryRef = useRef<RepositoryView | null>(null);
  const repositoryFolderRef = useRef(repositoryFolder);
  repositoryRef.current = repositoryView;
  useEffect(() => {
    let disposed = false;
    let stop = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    void watchRepositoryDirectory((id) => {
      if (disposed || repositoryRef.current?.repositoryId !== id || timer) return;
      timer = setTimeout(() => {
        timer = undefined;
        if (repositoryRef.current?.repositoryId === id)
          void repositoryControls.current
            ?.refresh(undefined, true)
            .catch((error) => alert(errorText(error), true));
      }, 500);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop();
      if (timer) clearTimeout(timer);
    };
  }, []);
  repositoryFolderRef.current = repositoryFolder;
  const [cryptFolder, setCryptFolder] = useState<{
    file: CloudFile;
    probe?: CryptFolderProbe;
    initialView?: CryptView;
    initialDirectory?: string;
    cloudPath?: string;
    route: Route;
  } | null>(null);
  const [managedCrypt, setManagedCrypt] = useState<{
    file: CloudFile;
    probe: CryptFolderProbe;
  } | null>(null);
  const [creatingCryptFolder, setCreatingCryptFolder] = useState(false);
  const [cryptEpoch, setCryptEpoch] = useState(0);
  const [cryptView, setCryptView] = useState<CryptView | null>(null);
  const [cryptBusy, setCryptBusy] = useState(false);
  const cryptControls = useRef<CryptControls>(null);
  const cryptManageFocus = useRef<HTMLButtonElement>(null);
  const cryptRef = useRef<CryptView | null>(null);
  const cryptFolderRef = useRef(cryptFolder);
  cryptRef.current = cryptView;
  cryptFolderRef.current = cryptFolder;
  useEffect(() => {
    let disposed = false;
    let stop = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;
    void watchCryptDirectory((folderId) => {
      if (disposed || cryptRef.current?.folderId !== folderId || timer) return;
      timer = setTimeout(() => {
        timer = undefined;
        if (cryptRef.current?.folderId === folderId)
          void cryptControls.current
            ?.refresh(undefined, true)
            .catch((error) => alert(errorText(error), true));
      }, 500);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    });
    return () => {
      disposed = true;
      stop();
      if (timer) clearTimeout(timer);
    };
  }, []);
  useEffect(() => {
    setRepositoryFolder(null);
    setRepositoryView(null);
    setRepositoryBusy(false);
    setCreatingRepository(false);
    setCryptFolder(null);
    setCryptView(null);
    setCryptBusy(false);
    setCreatingCryptFolder(false);
    setManagedCrypt(null);
  }, [account?.id, account?.space, account?.cloudId]);
  const [files, setFiles] = useState<CloudFile[]>(preview?.files ?? []);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [appBusy, setBusy] = useState(false);
  const busy = appBusy || repositoryBusy || cryptBusy;
  const [loadError, setLoadError] = useState('');
  const [query, setQuery] = useState('');
  const [mobileSearching, setMobileSearching] = useState(false);
  const [searchExpanded, setSearchExpanded] = useState(false);
  const searchInput = useRef<HTMLInputElement>(null);
  const searchButton = useRef<HTMLButtonElement>(null);
  const addressBarRef = useRef<AddressBarHandle>(null);
  useEffect(() => {
    if (searchExpanded) searchInput.current?.focus();
  }, [searchExpanded]);
  const closeExpandedSearch = useCallback(() => {
    setQuery('');
    setMobileSearching(false);
    setSearchExpanded(false);
    requestAnimationFrame(() => searchButton.current?.focus());
  }, []);
  const [viewMenuOpen, setViewMenuOpen] = useState(false);
  const deferredQuery = useDeferredValue(query);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [mobileSelecting, setMobileSelecting] = useState(false);
  const selectionAnchor = useRef<string | null>(null);
  const [sort, setSort] = useState({ key: 'name', desc: false });
  const [filter, setFilter] = useState('all');
  const [jobs, setJobs] = useState<TransferJob[]>(preview?.jobs ?? []);
  const activeQuotaAccount = useRef<string | null>(null);
  const [quota, setQuota] = useState<{ used: number; total: number } | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);
  const [view, setView] = useState<FileView>(() => {
    const saved = readPreference('cmcloud.fileView', 'list');
    return Object.hasOwn(fileViewLabels, saved) ? saved as FileView : 'list';
  });
  const [smallScreen, setSmallScreen] = useState(
    () => mobile || (typeof matchMedia === 'function' && matchMedia('(max-width: 700px)').matches),
  );
  const compact = !(
    mobile ||
    /Android|iPhone|iPad|iPod/i.test(navigator.userAgent) ||
    (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1)
  );
  const [fileAreaWidth, setFileAreaWidth] = useState(0);
  const [fileMenu, setFileMenu] = useState<{ x: number; y: number; files: CloudFile[] } | null>(
    null,
  );
  const mobileLongPress = useMobileFileLongPress(
    smallScreen,
    (id) => {
      setMobileSelecting(true);
      setSelection((current) => new Set([...current, id]));
      setFileMenu(null);
    },
    `${route}:${trail.at(-1)?.sourceId ?? ''}:${trail.at(-1)?.id}`,
  );
  const closeFileMenu = useCallback(() => setFileMenu(null), []);
  const [jobFilter, setJobFilter] = useState<TransferFilter>('active');
  const [cancelBatch, setCancelBatch] = useState<BatchDirection | null>(null);
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [overview, setOverview] = useState<Overview>(() => overviewFromJobs(preview?.jobs ?? []));
  const [hideVendorFolders, setHideVendorFolders] = useState(
    () => readPreference('cmcloud.hideVendorFolders', 'true') === 'true',
  );
  const [jobOffset, setJobOffset] = useState(0);
  const [jobSummary, setJobSummary] = useState<
    { status: string; direction?: string; count: number }[]
  >([]);
  const [scan, setScan] = useState<{ queued: number; path: string } | null>(null);
  const [previewFile, setPreviewFile] = useState<CloudFile | null>(null);
  const [transferDetail, setTransferDetail] = useState<TransferJob | null>(null);
  const browseTrail = useMemo(
    () =>
      repositoryView
        ? repositoryTrail(
            repositoryView.repositoryId,
            repositoryView.directory,
            repositoryFolder?.file.name ?? '对象库',
          )
        : cryptView
          ? cryptTrail(
              cryptView.folderId,
              cryptView.directory,
              cryptFolder?.file.name ?? '加密文件夹',
            )
          : trail,
    [
      repositoryView?.repositoryId,
      repositoryView?.directory,
      repositoryFolder?.file.name,
      cryptView?.folderId,
      cryptView?.directory,
      cryptFolder?.file.name,
      trail,
    ],
  );
  const parent = browseTrail[browseTrail.length - 1];
  const repositoryOps = useMemo(
    () => (repositoryView ? repositoryOperations(repositoryView) : undefined),
    [repositoryView],
  );
  const cryptOps = useMemo(
    () =>
      cryptView
        ? cryptOperations(cryptView, (next) => cryptControls.current?.acceptView(next))
        : undefined,
    [cryptView],
  );
  const activeOps = repositoryOps ?? cryptOps;
  const accountScope = `${account?.id}\0${account?.space}\0${account?.cloudId}`;
  const directoryKey = `${accountScope}\0${parent.sourceId ?? ''}\0${parent.id}`;
  const currentDirectory = useRef(directoryKey);
  currentDirectory.current = directoryKey;
  const generation = useRef(0);
  const pageLoading = useRef<number | null>(null);
  const removedFiles = useRef(new Set<string>());
  const changedFiles = useRef(new Map<string, CloudFile>());
  const listingLoaded = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const uploadDropRef = useRef<HTMLElement>(null);
  const recursiveSearch = useRecursiveFileSearch(
    activeOps?.list ?? api.list,
    `${directoryKey}/${route}/${hideVendorFolders}`,
    query,
  );
  const displayedFiles = recursiveSearch.state?.files ?? files;
  const selected = useMemo(
    () => displayedFiles.filter((f) => selection.has(f.id)),
    [displayedFiles, selection],
  );
  const visible = useMemo(
    () =>
      sortFiles(
        displayedFiles.filter(
          (f) =>
            (!hideVendorFolders ||
              !!repositoryView ||
              !!cryptView ||
              recursiveSearch.state !== null ||
              !isVendorFolder(f, trail.length === 1)) &&
            (!deferredQuery ||
              f.name.toLocaleLowerCase().includes(deferredQuery.toLocaleLowerCase())) &&
            (filter === 'all' || (filter === 'folders' && f.isDir) || fileKind(f) === filter),
        ),
        sort.key,
        sort.desc,
      ),
    [
      displayedFiles,
      deferredQuery,
      filter,
      sort,
      hideVendorFolders,
      trail.length,
      recursiveSearch.state,
      repositoryView,
      cryptView,
    ],
  );
  const grid = view === 'list' ? null : fileGridSizes[view];
  const columns = grid ? Math.max(1, Math.floor(fileAreaWidth / (smallScreen ? grid.mobileWidth : grid.width))) : 1;
  const rowHeight = grid?.height ?? (smallScreen || !compact ? 72 : 48);
  const virtual = useVirtualizer({
    count: Math.ceil(visible.length / columns),
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 12,
    getItemKey: (i) => visible[i * columns].id,
  });
  const visibleDirIds = virtual
    .getVirtualItems()
    .flatMap((row) => visible.slice(row.index * columns, (row.index + 1) * columns))
    .filter((file) => file.isDir)
    .map((file) => file.id);
  const [repositoryProbes, rememberRepositoryProbes] = useRepositoryDiscovery(
    visibleDirIds,
    `${accountScope}\0${trail[trail.length - 1].id}`,
    !!account &&
      (native || !!preview) &&
      account.space === 'personal_new' &&
      route === 'files' &&
      !repositoryFolder &&
      !cryptFolder,
    repositoryEpoch,
  );
  const [cryptProbes, rememberCryptProbes] = useCryptDiscovery(
    visibleDirIds,
    `${accountScope}\0${trail[trail.length - 1].sourceId ?? ''}\0${trail[trail.length - 1].id}`,
    !!account &&
      (native || !!preview) &&
      (route === 'files' || (route === 'shares' && !!parent.sourceId)) &&
      !repositoryFolder &&
      !cryptFolder,
    cryptEpoch,
  );
  const dragSelection = useFileDragSelection({
    scrollRef,
    files: visible,
    columns,
    rowHeight,
    selection,
    onSelection: setSelection,
    resetKey: `${directoryKey}/${route}/${view}`,
    desktopTouch: !smallScreen,
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
    const resize = () => setFileAreaWidth(area.clientWidth);
    resize();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', resize);
      return () => window.removeEventListener('resize', resize);
    }
    const observer = new ResizeObserver(resize);
    observer.observe(area);
    return () => observer.disconnect();
  }, [route, account?.id, smallScreen, parent.sourceId]);
  useEffect(() => setMobileSelecting(false), [directoryKey, route]);
  useEffect(() => {
    if (mobileSelecting && selection.size === 0) setMobileSelecting(false);
  }, [mobileSelecting, selection]);
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
  const acceptRepositoryView = useCallback((value: RepositoryView | null) => {
    if (!repositoryFolderRef.current) return;
    setRepositoryView(value);
    if (value) {
      setFiles(value.items.map((item) => repositoryFile(value.repositoryId, item)));
      setCursor(null);
      setHasMore(false);
      setLoadError('');
    }
  }, []);
  const closeRepository = useCallback(() => {
    const opened = repositoryRef.current;
    if (opened && opened.folderId === repositoryFolderRef.current?.file.id) {
      rememberRepositoryProbes([
        {
          folderId: opened.folderId,
          repositoryId: opened.repositoryId,
          kind: opened.encrypted ? 'encrypted' : 'plain',
          state: 'repository',
          saved: opened.saved,
        },
      ]);
    }
    generation.current++;
    repositoryFolderRef.current = null;
    setPreviewFile(null);
    repositoryRef.current = null;
    setRepositoryFolder(null);
    setRepositoryView(null);
    setRepositoryBusy(false);
    if (opened) {
      setFiles(preview?.files ?? []);
      setLoading(!!account && !preview);
    }
    setSelection(new Set());
    setQuery('');
  }, [account, preview, rememberRepositoryProbes]);
  const acceptCryptView = useCallback((value: CryptView | null) => {
    if (!cryptFolderRef.current) return;
    setCryptView(value);
    if (value) {
      setFiles(value.items.map((item) => cryptFile(value.folderId, item)));
      setCursor(null);
      setHasMore(false);
      setLoadError('');
    }
  }, []);
  const closeCrypt = useCallback(() => {
    const opened = cryptRef.current;
    if (opened && opened.folderId === cryptFolderRef.current?.file.id) {
      rememberCryptProbes([
        {
          folderId: opened.folderId,
          known: true,
          saved: opened.saved,
          options: opened.options,
        },
      ]);
    }
    generation.current++;
    cryptFolderRef.current = null;
    setPreviewFile(null);
    cryptRef.current = null;
    setCryptFolder(null);
    setCryptView(null);
    setCryptBusy(false);
    if (opened) {
      setFiles(preview?.files ?? []);
      setLoading((!!account || !!parent.sourceId) && !preview);
    }
    setSelection(new Set());
    setQuery('');
  }, [account, parent.sourceId, preview, rememberCryptProbes]);
  useEffect(() => {
    if (!native || mobile) return;
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => {
        void api.ready().catch((error) => alert(`托盘未能启动：${errorText(error)}`, true));
      });
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, [alert]);
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


  const load = useCallback(
    async (dir: CloudFile, next: string | null = null, append = false, merge = false) => {
      if (
        (repositoryFolderRef.current && !repositoryRef.current) ||
        (cryptFolderRef.current && !cryptRef.current)
      )
        return;
      if (pageLoading.current === generation.current) return;
      pageLoading.current = generation.current;
      setLoading(true);
      setLoadError('');
      const requestGeneration = generation.current;
      try {
        let page;
        const repository = repositoryRef.current;
        const crypt = cryptRef.current;
        if (repository) {
          const snapshot =
            dir.path === repository.directory && !merge
              ? repository
              : await repositoryControls.current?.refresh(dir.path, merge);
          if (!snapshot) return;
          page = {
            files: snapshot.items.map((item) => repositoryFile(snapshot.repositoryId, item)),
            nextCursor: null,
          };
        } else if (crypt) {
          const snapshot =
            dir.path === crypt.directory && !merge
              ? crypt
              : await cryptControls.current?.refresh(dir.path, merge);
          if (!snapshot) return;
          page = {
            files: snapshot.items.map((item) => cryptFile(snapshot.folderId, item)),
            nextCursor: null,
          };
        } else page = await api.list(dir, next);
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
      if (repositoryRef.current) return repositoryControls.current?.refresh(undefined, true);
      if (cryptRef.current) return cryptControls.current?.refresh(undefined, true);
      if ((account || parent.sourceId) && currentDirectory.current === directoryKey) {
        // Invalidate an in-flight page so a pre-mutation response cannot restore old entries.
        generation.current++;
        return load(parent, null, false, merge);
      }
    },
    [account, load, parent, directoryKey],
  );
  useEffect(() => {
    removedFiles.current.clear();
    changedFiles.current.clear();
    listingLoaded.current = false;
  }, [directoryKey]);
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
        parent.sourceId ||
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
  const emptySearch = !!query && visible.length === 0 && !loading && !loadError;
  const pullRefresh = usePullToRefresh(
    scrollRef,
    smallScreen &&
      (!!account || !!parent.sourceId) &&
      !mobileSelecting &&
      !emptySearch &&
      (route === 'files' || route === 'shares'),
    `${directoryKey}/${route}`,
    async () => {
      if (loading) return;
      recursiveSearch.clear();
      await refresh();
    },
  );
  useEffect(() => {
    setViewMenuOpen(false);
    setMobileSearching(false);
    setSearchExpanded(false);
    setManagedRepository(null);
    setManagedCrypt(null);
  }, [directoryKey, route]);
  const applyAccount = useCallback((value: AccountSummary) => {
    generation.current++;
    setAccount(value);
    setPersonalTrail([rootFile(value.rootId)]);
    setShareTrail([sharesRoot]);
    setQuota(null);
    setFiles([]);
    setSelection(new Set());
    setHasMore(false);
    setCursor(null);
    const accountId = value.id;
    activeQuotaAccount.current = accountId;
    void api
      .quota()
      .then((value) => {
        const q = value as Record<string, number>;
        if (q && activeQuotaAccount.current === accountId)
          setQuota({
            used: Number(q.used ?? q.usedSpace ?? 0),
            total: Number(q.total ?? q.totalSpace ?? 0),
          });
      })
      .catch(() => { if (activeQuotaAccount.current === accountId) setQuota(null); });
  }, []);
  useEffect(() => {
    if (!native) return;
    api
      .account()
      .then((value) => {
        if (value) applyAccount(value);
      })
      .catch((e) => alert(errorText(e), true))
      .finally(() => setRestoring(false));
  }, [applyAccount, alert]);
  useEffect(() => {
    if (
      !preview &&
      !cryptFolder &&
      ((route === 'files' && account && !repositoryFolder) ||
        (route === 'shares' && parent.sourceId))
    ) {
      const timer = setTimeout(() => void load(parent), 0);
      return () => clearTimeout(timer);
    }
  }, [account, parent, load, preview, repositoryFolder, cryptFolder, route]);
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
        .jobs(200, jobOffset, `${jobFilter}${errorsOnly && isOngoing(jobFilter) ? ':errors' : ''}`)
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
                  status: ['hashing', 'encrypting', 'decrypting', 'publishing'].includes(
                    progress.phase,
                  )
                    ? progress.phase
                    : progress.phase === 'completed' ? job.status : 'running',
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
  }, [jobOffset, jobFilter, errorsOnly]);
  useEffect(() => {
    if (!native || route !== 'transfers') return;
    let disposed = false;
    const refresh = () =>
      void api
        .overview()
        .then((value) => {
          if (!disposed) setOverview(value);
        })
        .catch(() => {});
    refresh();
    const timer = setInterval(refresh, 1000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [route]);
  const switchRoute = (next: Route) => {
    if (next === route) {
      setPreviewFile(null);
      return;
    }
    if (next === 'shares' && repositoryFolder) closeRepository();
    if (cryptFolder && (next === 'shares' || route === 'shares')) closeCrypt();
    if (next === 'shares' || route === 'shares') {
      generation.current++;
      setFiles([]);
      setLoadError('');
      setCursor(null);
      setHasMore(false);
      setLoading(false);
      setSelection(new Set());
      setMobileSelecting(false);
      setQuery('');
      setFileMenu(null);
    }
    setPreviewFile(null);
    setRoute(next);
  };
  const openBrowseSource = (source: BrowseSource) => {
    if (cryptFolder) closeCrypt();
    generation.current++;
    setFiles([]);
    setLoadError('');
    setCursor(null);
    setHasMore(false);
    setLoading(true);
    setSelection(new Set());
    setMobileSelecting(false);
    setQuery('');
    setShareTrail([sharesRoot, { ...rootFile(''), name: source.name, sourceId: source.id }]);
  };
  const openFolder = (file: CloudFile) => {
    if (repositoryRef.current) {
      setSelection(new Set());
      if (recursiveSearch.state) setQuery('');
      void run(async () => {
        await repositoryControls.current?.refresh(file.path, false);
      });
      return;
    }
    if (cryptRef.current) {
      setSelection(new Set());
      if (recursiveSearch.state) setQuery('');
      void run(async () => {
        await cryptControls.current?.refresh(file.path, false);
      });
      return;
    }
    setRepositoryFolder(null);
    setCryptFolder(null);
    generation.current++;
    setFiles([]);
    setSelection(new Set());
    setHasMore(false);
    if (recursiveSearch.state) setQuery('');
    setTrail([...trail, ...(recursiveSearch.state?.parents.get(file.id) ?? []), file]);
  };
  const goTo = (index: number) => {
    if (repositoryRef.current) {
      setSelection(new Set());
      setQuery('');
      void run(async () => {
        await repositoryControls.current?.refresh(browseTrail[index].path, false);
      });
      return;
    }
    if (cryptRef.current) {
      setSelection(new Set());
      setQuery('');
      void run(async () => {
        await cryptControls.current?.refresh(browseTrail[index].path, false);
      });
      return;
    }
    generation.current++;
    setFiles([]);
    setSelection(new Set());
    setHasMore(false);
    setTrail(trail.slice(0, index + 1));
  };
  useEffect(() => {
    const back = (event: Event) => {
      const dialog = document.querySelector<HTMLDialogElement>('dialog[open]');
      if (dialog) {
        dialog.dispatchEvent(new Event('cancel', { cancelable: true }));
        event.preventDefault();
        return;
      }
      if (cancelBatch) {
        if (!busy) setCancelBatch(null);
        event.preventDefault();
        return;
      }
      if (viewMenuOpen) setViewMenuOpen(false);
      else if (prompt) setPrompt(null);
      else if (previewFile) {
        if (window.dispatchEvent(new Event('cmcloud-preview-back', { cancelable: true }))) setPreviewFile(null);
      }
      else if (transferDetail) setTransferDetail(null);
      else if (fileMenu) closeFileMenu();
      else if (addressBarRef.current?.isEditing()) addressBarRef.current.cancel();
      else if (mobileSelecting || selection.size > 0) {
        setMobileSelecting(false);
        setSelection(new Set());
      } else if (searchExpanded) closeExpandedSearch();
      else if (recursiveSearch.state) recursiveSearch.clear();
      else if (repositoryFolder && route === 'files')
        window.dispatchEvent(new Event('cmcloud-repository-back', { cancelable: true }));
      else if (cryptFolder && (route === 'files' || route === 'shares'))
        window.dispatchEvent(new Event('cmcloud-crypt-back', { cancelable: true }));
      else if ((route === 'files' || route === 'shares') && trail.length > 1)
        goTo(trail.length - 2);
      else if (route === 'account') setRoute('settings');
      else if (route !== 'files') switchRoute('files');
      else return;
      event.preventDefault();
    };
    window.addEventListener('cmcloud-back', back);
    return () => window.removeEventListener('cmcloud-back', back);
  }, [
    cancelBatch,
    repositoryFolder,
    cryptFolder,
    busy,
    viewMenuOpen,
    mobileSearching,
    searchExpanded,
    closeExpandedSearch,
    prompt,
    previewFile,
    transferDetail,
    fileMenu,
    mobileSelecting,
    selection,
    recursiveSearch.state,
    trail,
    route,
  ]);
  useEffect(() => {
    if (!previewFile) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented || document.querySelector('dialog[open]')) return;
      if (window.dispatchEvent(new Event('cmcloud-preview-back', { cancelable: true }))) setPreviewFile(null);
      event.preventDefault();
    };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, [previewFile]);
  useEffect(() => {
    const onShortcut = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        (route !== 'files' && route !== 'shares') ||
        previewFile ||
        document.querySelector('dialog[open]')
      )
        return;
      const key = event.key.toLowerCase();
      if (((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && key === 'l') ||
          (event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey && key === 'd')) {
        event.preventDefault();
        addressBarRef.current?.edit();
      }
    };
    window.addEventListener('keydown', onShortcut);
    return () => window.removeEventListener('keydown', onShortcut);
  }, [route, previewFile]);
  const toggle = (file: CloudFile) => {
    const next = new Set(selection);
    if (next.has(file.id)) next.delete(file.id);
    else next.add(file.id);
    setSelection(next);
    if (next.size === 0) setMobileSelecting(false);
  };
  const refreshTransferRecords = async (offset: number) => {
    const summary = await api.summary();
    const total = summary.filter((job) => matchesTransfer(job, jobFilter, errorsOnly && isOngoing(jobFilter)))
      .reduce((sum, job) => sum + job.count, 0);
    const page = Math.min(offset, Math.floor(Math.max(total - 1, 0) / 200) * 200);
    const filter = jobFilter + (errorsOnly && isOngoing(jobFilter) ? ':errors' : '');
    setJobSummary(summary);
    setJobOffset(page);
    setJobs(await api.jobs(200, page, filter));
  };
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
      const repository = repositoryRef.current;
      if (repository) {
        const results = await repositoryApi.queueDownloads(
          repository.handle,
          repository.revision,
          targets.map((file) => file.path),
        );
        if (results.length) {
          setJobs((previous) => [...results, ...previous].slice(0, 200));
          alert('下载任务已加入传输队列');
        }
        return;
      }
      const crypt = cryptRef.current;
      if (crypt) {
        setScan({ queued: 0, path: '正在准备解密下载' });
        try {
          const results = await cryptApi.queueDownloads(
            crypt.handle,
            targets.map((file) => file.path),
          );
          if (results.length) {
            setJobs((previous) => [...results, ...previous].slice(0, 200));
            alert('解密下载任务已加入传输队列');
          }
        } finally {
          setScan(null);
        }
        return;
      }
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
      if (repositoryRef.current) {
        await repositoryControls.current?.upload('files');
        return;
      }
      if (cryptRef.current) {
        const result = await cryptControls.current?.upload('files');
        if (result?.queued) {
          alert(`${result.queued} 个文件已加入加密上传队列`);
        }
        return;
      }
      const results = await api.upload(parent);
      if (results.length) {
        setJobs((prev) => [...results, ...prev].slice(0, 200));
        alert(`${results.length} 个文件已加入上传队列`);
      }
    });
  const uploadFolder = () =>
    void run(async () => {
      if (repositoryRef.current) {
        await repositoryControls.current?.upload('directory');
        return;
      }
      if (cryptRef.current) {
        setScan({ queued: 0, path: '选择并扫描加密文件夹' });
        try {
          const result = await cryptControls.current?.upload('directory');
          if (result?.queued)
            alert(
              `${result.queued.toLocaleString()} 个文件已加入加密上传队列${result.skipped ? `，跳过 ${result.skipped} 个特殊文件` : ''}`,
            );
        } finally {
          setScan(null);
          refresh(true);
        }
        return;
      }
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
  const batchSummary = native
    ? jobSummary
    : jobs.map((job) => ({ status: job.status, direction: job.direction, count: 1 }));
  useEffect(() => setCancelBatch(null), [route, account?.id, account?.space, account?.cloudId]);
  const runBatch = (action: BatchAction, direction: BatchDirection) =>
    void run(async () => {
      const count = await api.controlBatch(action, direction);
      setCancelBatch(null);
      setJobOffset(0);
      const updates = await Promise.allSettled([
        api.jobs(200, 0, `${jobFilter}${errorsOnly && isOngoing(jobFilter) ? ':errors' : ''}`),
        ...(native ? [api.summary(), api.overview()] : []),
      ]);
      if (updates[0].status === 'fulfilled') setJobs(updates[0].value as TransferJob[]);
      if (native && updates[1]?.status === 'fulfilled')
        setJobSummary(updates[1].value as typeof jobSummary);
      if (native && updates[2]?.status === 'fulfilled') setOverview(updates[2].value as Overview);
      alert(
        count > 0
          ? `${count.toLocaleString()} 个任务已${action === 'pause' ? '暂停' : action === 'resume' ? '加入开始队列' : '取消'}`
          : action === 'resume'
            ? '当前账号和空间没有可开始的任务'
            : '没有可操作的任务',
      );
    });
  const showLink = (file: CloudFile) =>
    void run(async () => {
      const repository = repositoryRef.current;
      if (repository) {
        const link = await repositoryApi.link(repository.handle, repository.revision, file.path);
        await navigator.clipboard.writeText(link.url);
        alert(
          link.encrypted
            ? '加密对象链接已复制；文件须在对象库中解密'
            : '下载链接已复制；链接有效期由云盘服务决定',
        );
        return;
      }
      const link = await api.link(file);
      await navigator.clipboard.writeText(link);
      alert('下载链接已复制；链接有效期由云盘服务决定');
    });
  const activeJobs = jobs.filter((j) =>
    [
      'running',
      'queued',
      'hashing',
      'encrypting',
      'decrypting',
      'publishing',
      'downloading',
      'uploading',
    ].includes(j.status),
  );
  const activeCount = native
    ? jobSummary
        .filter((j) => ['running', 'queued'].includes(j.status))
        .reduce((count, j) => count + j.count, 0)
    : activeJobs.length;
  const jobTotal = native
    ? jobSummary
        .filter((job) => matchesTransfer(job, jobFilter, errorsOnly && isOngoing(jobFilter)))
        .reduce((sum, job) => sum + job.count, 0)
    : jobs.filter((job) => matchesTransfer(job, jobFilter, errorsOnly && isOngoing(jobFilter)))
        .length;
  const readOnly = route === 'shares' || !!repositoryView?.historical;
  const dropCount = useFileUploadDrop({
    area: uploadDropRef,
    enabled:
      native &&
      !mobile &&
      !!account &&
      !readOnly &&
      route === 'files' &&
      (!repositoryFolder || !!repositoryView) &&
      (!cryptFolder || !!cryptView) &&
      !busy &&
      !scan &&
      !prompt &&
      !fileMenu &&
      !viewMenuOpen,
    scope: directoryKey,
    watch: native && !mobile ? watchFileDrop : undefined,
    onError: (error) => alert(`拖拽上传失败：${errorText(error)}`, true),
    onDrop: (paths) =>
      run(async () => {
        if (repositoryRef.current) {
          await repositoryControls.current?.upload('files', paths);
          return;
        }
        if (cryptRef.current) {
          setScan({ queued: 0, path: '正在扫描拖入的文件' });
          try {
            const result = await cryptControls.current?.upload('directory', paths);
            if (result?.queued)
              alert(
                `${result.queued.toLocaleString()} 个文件已加入加密上传队列${result.skipped ? `，跳过 ${result.skipped} 个特殊文件` : ''}`,
              );
          } finally {
            setScan(null);
            refresh(true);
          }
          return;
        }
        setScan({ queued: 0, path: '正在扫描拖入的文件' });
        try {
          const result = await api.uploadPaths(parent, paths);
          alert(
            `${result.queued.toLocaleString()} 个文件已加入上传队列${result.skipped ? `，跳过 ${result.skipped} 个特殊文件` : ''}`,
          );
        } finally {
          setScan(null);
          refresh(true);
        }
      }),
  });
  const openAsCryptFolder = (file: CloudFile, probe?: CryptFolderProbe, initialDirectory?: string) => {
    const searchParents = recursiveSearch.state?.parents.get(file.id) ?? [];
    const cloudPath = formatCloudPath([...trail.slice(1), ...searchParents, file]);
    if (repositoryFolderRef.current) closeRepository();
    if (cryptFolderRef.current) closeCrypt();
    generation.current++;
    setSelection(new Set());
    setMobileSelecting(false);
    setCryptFolder({ file, probe, cloudPath, initialDirectory, route });
  };
  const openFile = (file: CloudFile) => {
    if (!file.isDir && (contentKind(file) || ['视频', '图片', '音频', 'PDF'].includes(fileKind(file)))) {
      setPreviewFile(file);
      return;
    }
    if (repositoryRef.current || cryptRef.current) {
      if (file.isDir) openFolder(file);
      else alert('此文件不支持预览，请下载后打开。');
      return;
    }
    if (file.isDir) {
      if (!native && !preview) {
        openFolder(file);
        return;
      }
      if (cryptProbes[file.id]?.known) {
        openAsCryptFolder(file, cryptProbes[file.id]);
        return;
      }
      if (readOnly || account?.space !== 'personal_new') {
        openFolder(file);
        return;
      }
      void run(async () => {
        const requestGeneration = generation.current;
        if (!cryptProbes[file.id]) {
          const cProbe = (await cryptApi.probe([file.id]).catch(() => []))[0];
          if (generation.current !== requestGeneration) return;
          if (cProbe) rememberCryptProbes([cProbe]);
          if (cProbe?.known) {
            openAsCryptFolder(file, cProbe);
            return;
          }
        }
        const cached = repositoryProbes[file.id];
        const probe = cached ?? (await repositoryApi.probe([file.id]))[0];
        if (generation.current !== requestGeneration) return;
        if (!cached && probe) rememberRepositoryProbes([probe]);
        if (probe?.state === 'repository') {
          const searchParents = recursiveSearch.state?.parents.get(file.id) ?? [];
          const cloudPath = formatCloudPath([...trail.slice(1), ...searchParents, file]);
          generation.current++;
          setSelection(new Set());
          setRepositoryFolder({ file, probe, cloudPath });
        } else if (probe?.state === 'ordinary') openFolder(file);
        else alert('暂时无法验证此文件夹。请刷新后重试，或从菜单选择“以普通文件夹打开”。', true);
      });
    } else alert('此文件不支持预览，请下载后打开。');
  };
  const openFileMenu = (file: CloudFile, x: number, y: number) => {
    if (busy) return;
    const targets = selection.has(file.id) ? selected : [file];
    if (!smallScreen && !selection.has(file.id)) setSelection(new Set([file.id]));
    setFileMenu({ x, y, files: targets });
  };
  const repositoryCloudPath = repositoryFolder
    ? (repositoryFolder.cloudPath ?? formatCloudPath([...trail.slice(1), repositoryFolder.file]))
    : '';
  const cryptCloudPath = cryptFolder
    ? (cryptFolder.cloudPath ?? formatCloudPath([...trail.slice(1), cryptFolder.file]))
    : '';
  const currentDisplayPath = repositoryView
    ? formatRepositoryPath(repositoryCloudPath, repositoryView.directory)
    : cryptView
      ? formatRepositoryPath(cryptCloudPath, cryptView.directory)
      : formatCloudPath(trail.slice(1));
  const itemDisplayPath = (file: CloudFile) =>
    repositoryView
      ? formatRepositoryPath(repositoryCloudPath, file.path)
      : cryptView
        ? formatRepositoryPath(cryptCloudPath, file.path)
        : formatCloudPath([
            ...trail.slice(1),
            ...(recursiveSearch.state?.parents.get(file.id) ?? []),
            file,
          ]);
  const copyPath = (value: string, message: string) => {
    void navigator.clipboard
      .writeText(value)
      .then(() => alert(message))
      .catch((e) => alert(errorText(e), true));
  };
  const navigateToPath = async (rawPath: string): Promise<boolean> => {
    const parsed = parseAddressPath(rawPath, {
      cloudSegments: trail.slice(1).map((item) => item.name),
      repository: repositoryView
        ? {
            cloudSegments: repositoryCloudPath.split('/').filter(Boolean),
            innerPath: repositoryView.directory,
          }
        : cryptView
          ? {
              cloudSegments: cryptCloudPath.split('/').filter(Boolean),
              innerPath: cryptView.directory,
            }
          : undefined,
    });
    const resolveCloudChild = async (dir: CloudFile, name: string) => {
      if (!repositoryFolder && !cryptFolder && dir.id === parent.id && files.length > 0 && !hasMore) {
        const local = files.find((item) => item.name === name);
        if (local) return { entry: local, page: null };
      }
      const firstPage = await api.list(dir, null);
      let found = firstPage.files.find((item) => item.name === name) ?? null;
      let next = firstPage.nextCursor ?? null;
      let guard = 0;
      while (!found && next && guard < 50) {
        guard++;
        const page = await api.list(dir, next);
        found = page.files.find((item) => item.name === name) ?? null;
        next = page.nextCursor ?? null;
      }
      return { entry: found, page: firstPage };
    };
    const applyCloudDestination = (
      nextTrail: CloudFile[],
      page: { files: CloudFile[]; nextCursor?: string | null } | null,
      selectedIds = new Set<string>(),
    ) => {
      if (repositoryFolderRef.current) closeRepository();
      if (cryptFolderRef.current) closeCrypt();
      generation.current++;
      if (recursiveSearch.state) setQuery('');
      setMobileSelecting(false);
      setSelection(selectedIds);
      if (page) {
        setFiles(page.files);
        setCursor(page.nextCursor ?? null);
        setHasMore(!!page.nextCursor);
        listingLoaded.current = true;
        setLoadError('');
      } else {
        setFiles([]);
        setCursor(null);
        setHasMore(false);
      }
      setTrail(nextTrail);
    };

    if (parsed.kind === 'repository') {
      const targetRepoCloudPath = formatCloudPath(parsed.cloudSegments.map((name) => ({ name })));
      if (cryptView && targetRepoCloudPath === cryptCloudPath) {
        if (parsed.innerPath === cryptView.directory) return true;
        setBusy(true);
        try {
          if (!parsed.innerPath) {
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            await cryptControls.current?.refresh('', false);
            return true;
          }
          try {
            const sub = await cryptApi.list(cryptView.handle, parsed.innerPath, false);
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            cryptControls.current?.acceptView(sub);
            return true;
          } catch {
            const slash = parsed.innerPath.lastIndexOf('/');
            const parentDir = slash < 0 ? '' : parsed.innerPath.slice(0, slash);
            const leaf = slash < 0 ? parsed.innerPath : parsed.innerPath.slice(slash + 1);
            const parentView =
              parentDir === cryptView.directory
                ? cryptView
                : await cryptApi.list(cryptView.handle, parentDir, false);
            const match = parentView.items.find((item) => item.name === leaf);
            if (match) {
              setMobileSelecting(false);
              setQuery('');
              if (parentView !== cryptView) cryptControls.current?.acceptView(parentView);
              const targetFile = cryptFile(parentView.folderId, match);
              setSelection(new Set([targetFile.id]));
              alert(`已定位到 ${match.name}`);
              return true;
            }
            alert(`路径不存在：${formatRepositoryPath(targetRepoCloudPath, parsed.innerPath)}`, true);
            return false;
          }
        } catch (e) {
          alert(errorText(e), true);
          return false;
        } finally {
          setBusy(false);
        }
      }
      if (repositoryView && targetRepoCloudPath === repositoryCloudPath) {
        if (parsed.innerPath === repositoryView.directory) return true;
        setBusy(true);
        try {
          if (!parsed.innerPath) {
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            await repositoryControls.current?.refresh('', false);
            return true;
          }
          const sub = await repositoryApi.list(repositoryView.handle, parsed.innerPath, false);
          if (sub.items.length > 0 || sub.conflicts.length > 0) {
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            repositoryControls.current?.acceptView(sub);
            return true;
          }
          const slash = parsed.innerPath.lastIndexOf('/');
          const parentDir = slash < 0 ? '' : parsed.innerPath.slice(0, slash);
          const leaf = slash < 0 ? parsed.innerPath : parsed.innerPath.slice(slash + 1);
          const parentView =
            parentDir === repositoryView.directory
              ? repositoryView
              : await repositoryApi.list(repositoryView.handle, parentDir, false);
          const match = parentView.items.find((item) => item.name === leaf);
          if (match) {
            setMobileSelecting(false);
            setQuery('');
            if (parentView !== repositoryView) repositoryControls.current?.acceptView(parentView);
            const targetFile = repositoryFile(parentView.repositoryId, match);
            setSelection(new Set([targetFile.id]));
            alert(`已定位到 ${match.name}`);
            return true;
          }
          alert(`路径不存在：${formatRepositoryPath(targetRepoCloudPath, parsed.innerPath)}`, true);
          return false;
        } catch (e) {
          alert(errorText(e), true);
          return false;
        } finally {
          setBusy(false);
        }
      }

      if (!native && !preview) {
        alert('当前环境不支持打开对象库或加密文件夹', true);
        return false;
      }
      if (!parsed.cloudSegments.length) {
        alert('路径无效', true);
        return false;
      }
      setBusy(true);
      try {
        let common = 0;
        while (
          common < parsed.cloudSegments.length &&
          trail[common + 1]?.name === parsed.cloudSegments[common] &&
          trail[common + 1]?.isDir
        ) {
          common++;
        }
        const resolved = trail.slice(0, common + 1);
        let curDir = resolved[resolved.length - 1];
        for (let i = common; i < parsed.cloudSegments.length; i++) {
          const { entry } = await resolveCloudChild(curDir, parsed.cloudSegments[i]);
          if (!entry) {
            alert(`路径不存在：${targetRepoCloudPath}`, true);
            return false;
          }
          if (!entry.isDir) {
            alert(`路径不是文件夹：/${parsed.cloudSegments.slice(0, i + 1).join('/')}`, true);
            return false;
          }
          resolved.push(entry);
          curDir = entry;
        }
        const repoDir = resolved[resolved.length - 1];
        const cProbe =
          cryptProbes[repoDir.id] ?? (await cryptApi.probe([repoDir.id]).catch(() => []))[0];
        if (cProbe) rememberCryptProbes([cProbe]);
        if (cProbe?.known) {
          if (repositoryFolderRef.current) closeRepository();
          if (cryptFolderRef.current) closeCrypt();
          generation.current++;
          setTrail(resolved.slice(0, -1));
          setSelection(new Set());
          setMobileSelecting(false);
          setQuery('');
          setCryptFolder({
            file: repoDir,
            probe: cProbe,
            cloudPath: targetRepoCloudPath,
            initialDirectory: parsed.innerPath || undefined,
            route,
          });
          return true;
        }
        if (route !== 'files' || account?.space !== 'personal_new') {
          alert('当前空间不支持打开对象库；如需打开加密文件夹请先从菜单选择“以加密文件夹打开”', true);
          return false;
        }
        const probe = repositoryProbes[repoDir.id] ?? (await repositoryApi.probe([repoDir.id]))[0];
        if (probe) rememberRepositoryProbes([probe]);
        if (probe?.state !== 'repository') {
          alert(`指定目录不是已识别的对象库或加密文件夹：${targetRepoCloudPath}`, true);
          return false;
        }
        if (repositoryFolderRef.current) closeRepository();
        if (cryptFolderRef.current) closeCrypt();
        generation.current++;
        setTrail(resolved.slice(0, -1));
        setSelection(new Set());
        setMobileSelecting(false);
        setQuery('');
        setRepositoryFolder({
          file: repoDir,
          probe,
          cloudPath: targetRepoCloudPath,
          initialDirectory: parsed.innerPath || undefined,
        });
        return true;
      } catch (e) {
        alert(errorText(e), true);
        return false;
      } finally {
        setBusy(false);
      }
    }

    if (!parsed.segments.length) {
      if (!repositoryFolder && !cryptFolder && trail.length === 1) return true;
      applyCloudDestination([trail[0]], preview ? { files: preview.files } : null);
      return true;
    }

    let common = 0;
    while (
      common < parsed.segments.length &&
      trail[common + 1]?.name === parsed.segments[common] &&
      trail[common + 1]?.isDir
    ) {
      common++;
    }
    if (common === parsed.segments.length) {
      if (!repositoryFolder && !cryptFolder && trail.length === common + 1) return true;
      setBusy(true);
      try {
        const targetTrail = trail.slice(0, common + 1);
        const targetPage = preview
          ? await api.list(targetTrail[targetTrail.length - 1], null)
          : null;
        applyCloudDestination(targetTrail, targetPage);
        return true;
      } catch (e) {
        alert(errorText(e), true);
        return false;
      } finally {
        setBusy(false);
      }
    }

    setBusy(true);
    try {
      const resolved = trail.slice(0, common + 1);
      let curDir = resolved[resolved.length - 1];
      for (let i = common; i < parsed.segments.length; i++) {
        const { entry, page } = await resolveCloudChild(curDir, parsed.segments[i]);
        if (!entry) {
          alert(`路径不存在：/${parsed.segments.join('/')}`, true);
          return false;
        }
        if (i < parsed.segments.length - 1) {
          if (!entry.isDir) {
            alert(`路径不是文件夹：/${parsed.segments.slice(0, i + 1).join('/')}`, true);
            return false;
          }
          resolved.push(entry);
          curDir = entry;
          continue;
        }
        if (!entry.isDir) {
          if (curDir.id === parent.id && !repositoryFolder && !cryptFolder) {
            setMobileSelecting(false);
            setSelection(new Set([entry.id]));
          } else {
            const parentPage = page ?? (await api.list(curDir, null));
            applyCloudDestination(resolved, parentPage, new Set([entry.id]));
          }
          alert(`已定位到 ${entry.name}`);
          return true;
        }
        if (native || !!preview) {
          const cProbe =
            cryptProbes[entry.id] ?? (await cryptApi.probe([entry.id]).catch(() => []))[0];
          if (cProbe) rememberCryptProbes([cProbe]);
          if (cProbe?.known) {
            if (repositoryFolderRef.current) closeRepository();
            if (cryptFolderRef.current) closeCrypt();
            generation.current++;
            setTrail(resolved);
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            setCryptFolder({
              file: entry,
              probe: cProbe,
              cloudPath: `/${parsed.segments.join('/')}`,
              route,
            });
            return true;
          }
        }
        if (route === 'files' && !readOnly && account?.space === 'personal_new' && (native || !!preview)) {
          const probe = repositoryProbes[entry.id] ?? (await repositoryApi.probe([entry.id]))[0];
          if (probe) rememberRepositoryProbes([probe]);
          if (probe?.state === 'repository') {
            if (repositoryFolderRef.current) closeRepository();
            if (cryptFolderRef.current) closeCrypt();
            generation.current++;
            setTrail(resolved);
            setSelection(new Set());
            setMobileSelecting(false);
            setQuery('');
            setRepositoryFolder({
              file: entry,
              probe,
              cloudPath: `/${parsed.segments.join('/')}`,
            });
            return true;
          }
        }
        const targetPage = preview ? await api.list(entry, null) : null;
        applyCloudDestination([...resolved, entry], targetPage);
        return true;
      }
      return true;
    } catch (e) {
      alert(errorText(e), true);
      return false;
    } finally {
      setBusy(false);
    }
  };
  const menuItems: FileMenuItem[] = [];
  if (fileMenu && fileMenu.files.length === 0) {
    menuItems.push(
      { label: '刷新', icon: RefreshCw, action: () => refresh(), disabled: loading },
      {
        label: '复制当前路径',
        icon: Copy,
        action: () => copyPath(currentDisplayPath, '当前路径已复制'),
      },
    );
    if (!readOnly)
      menuItems.push(
        { label: '上传文件夹', icon: FolderInput, action: uploadFolder, disabled: busy },
        { label: '上传文件', icon: Upload, action: upload, disabled: busy },
        {
          label: '新建文件夹',
          icon: FolderPlus,
          action: () => setPrompt({ kind: 'mkdir', files: [] }),
          disabled: busy,
        },
      );
    if (!readOnly && account?.space === 'personal_new' && !repositoryFolder && !cryptFolder)
      menuItems.push({
        label: '新建对象库',
        icon: HardDrive,
        action: () => setCreatingRepository(true),
        disabled: busy,
      });
    if (!readOnly && !repositoryFolder && !cryptFolder)
      menuItems.push({
        label: '新建加密文件夹',
        icon: ShieldCheck,
        action: () => setCreatingCryptFolder(true),
        disabled: busy,
      });
    if (repositoryView)
      menuItems.push(
        {
          label: '管理对象库',
          icon: Settings,
          action: () => repositoryControls.current?.manage(),
          disabled: busy,
        },
        { label: '退出对象库', icon: LogOut, action: closeRepository, disabled: busy },
      );
    if (cryptView)
      menuItems.push(
        {
          label: '加密文件夹设置',
          icon: Settings,
          action: () => cryptControls.current?.manage(),
          disabled: busy,
        },
        { label: '退出加密文件夹', icon: LogOut, action: closeCrypt, disabled: busy },
      );
  } else if (fileMenu) {
    const targets = fileMenu.files;
    const single = targets.length === 1 ? targets[0] : null;
    const isKnownCrypt = !!(single?.isDir && !repositoryView && !cryptView && cryptProbes[single.id]?.known);
    const isKnownRepo = !!(single?.isDir && !readOnly && repositoryProbes[single.id]?.state === 'repository');
    if (single)
      menuItems.push({
        label: single.isDir
          ? isKnownRepo
            ? '打开 AxuanStor 对象库'
            : isKnownCrypt
              ? '打开加密文件夹'
              : '打开文件夹'
          : fileKind(single) === '视频'
            ? '视频预览'
            : contentKind(single) || ['图片', '音频', 'PDF'].includes(fileKind(single))
              ? '打开预览'
              : '打开文件',
        icon: single.isDir ? (isKnownCrypt ? ShieldCheck : Folder) : ExternalLink,
        action: () => openFile(single),
      });
    if (single?.isDir && account?.space === 'personal_new' && !repositoryFolder && !cryptFolder) {
      if (!readOnly && repositoryProbes[single.id]?.state === 'repository')
        menuItems.push({
          label: '管理对象库',
          icon: Settings,
          action: () => setManagedRepository({ file: single, probe: repositoryProbes[single.id] }),
        });
      const state = repositoryProbes[single.id]?.state;
      if (state === 'repository' || state === 'unavailable')
        menuItems.push({ label: '以普通文件夹打开', icon: Folder, action: () => openFolder(single) });
    }
    if (single?.isDir && !repositoryFolder && !cryptFolder && repositoryProbes[single.id]?.state !== 'repository') {
      if (cryptProbes[single.id]?.known) {
        menuItems.push(
          {
            label: '加密文件夹设置',
            icon: Settings,
            action: () => setManagedCrypt({ file: single, probe: cryptProbes[single.id] }),
          },
          { label: '以普通文件夹打开', icon: Folder, action: () => openFolder(single) },
        );
      } else if (native || !!preview) {
        menuItems.push({
          label: '以加密文件夹打开',
          icon: ShieldCheck,
          action: () => openAsCryptFolder(single, cryptProbes[single.id]),
        });
      }
    }
    menuItems.push({ label: '下载', icon: Download, action: () => download(targets) });
    if (single)
      menuItems.push({
        label: single.isDir ? '复制文件夹路径' : '复制文件路径',
        icon: Copy,
        action: () =>
          copyPath(
            itemDisplayPath(single),
            single.isDir ? '文件夹路径已复制' : '文件路径已复制',
          ),
      });
    if (single?.isDir && single.id && !repositoryView && !cryptView)
      menuItems.push({
        label: '复制目录 ID',
        icon: Copy,
        action: () => void run(() => navigator.clipboard.writeText(single.id), '目录 ID 已复制'),
      });
    if (single && !single.isDir && !cryptView)
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
    { route: 'shares', label: '群组与分享', icon: Users },
  ];

  const mobileNav = [...nav, { route: 'settings' as Route, label: '设置', icon: Settings }];

  return (
    <div
      className={`app-shell ${smallScreen ? 'mobile-layout' : ''} ${collapsed ? 'sidebar-collapsed' : ''}`}
    >
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <svg viewBox="0 0 36 36" width="26" height="26" aria-hidden="true">
              <path
                d="M17.8 24.8H10.2a5.1 5.1 0 0 1-1.8-9.85 8.1 8.1 0 0 1 15.4-1.4 4.8 4.8 0 0 1 4.1 2.45"
                fill="none"
                stroke="#fff"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              <path
                d="M12.8 14.3a4.6 4.6 0 0 1 7-1"
                fill="none"
                stroke="#bae6fd"
                strokeOpacity="0.75"
                strokeWidth="1.35"
                strokeLinecap="round"
              />
              <path
                d="M22.8 22v4M20.8 24h4"
                fill="none"
                stroke="#fff"
                strokeWidth="2.1"
                strokeLinecap="round"
              />
              <path
                d="M28.8 18.8v4M26.8 20.8h4"
                fill="none"
                stroke="#7dd3fc"
                strokeWidth="2.1"
                strokeLinecap="round"
              />
            </svg>
          </span>
          <span>
            CMCloud<span className="brand-plus">++</span>
          </span>
        </div>
        <div className="nav-group">
          <button
            className="nav-item sidebar-toggle"
            aria-label={collapsed ? '展开侧栏' : '收起'}
            title={collapsed ? '展开侧栏' : '收起'}
            aria-expanded={!collapsed}
            onClick={() => {
              setCollapsed(!collapsed);
              savePreference('cmcloud.sidebar', !collapsed ? 'collapsed' : 'expanded');
            }}
          >
            {collapsed ? <PanelLeftOpen size={20} /> : <PanelLeftClose size={20} />}
            <span>收起</span>
          </button>
          {nav.map(({ route: r, label, icon: Icon }) => (
            <button
              key={r}
              title={label}
              aria-label={label}
              aria-current={route === r ? 'page' : undefined}
              className={`nav-item ${route === r ? 'active' : ''}`}
              onClick={() => switchRoute(r)}
            >
              <Icon size={20} />
              <span>{label}</span>
              {r === 'transfers' && activeCount > 0 && <span className="count">{activeCount}</span>}
            </button>
          ))}
        </div>
        <nav className="sidebar-utilities" aria-label="会话与偏好">
          <button
            className={`nav-item ${route === 'settings' || route === 'account' ? 'active' : ''}`}
            title="设置"
            aria-label="设置"
            aria-current={route === 'settings' || route === 'account' ? 'page' : undefined}
            onClick={() => switchRoute('settings')}
          >
            <Settings size={20} />
            <span>设置</span>
          </button>
        </nav>
      </aside>
      <div className={'main-shell' + (previewFile ? ' preview-active' : '')} ref={mainRef}>
        {preview && (
          <p className="preview-fixture-banner">
            示例数据 · 界面预览
          </p>
        )}
        {previewFile && contentKind(previewFile) ? (
          <FileContentPreview
            key={previewFile.id}
            file={previewFile}
            kind={contentKind(previewFile)!}
            smallScreen={smallScreen}
            initialFileView={view}
            repository={previewFile.id.startsWith('repository:') ? repositoryView ?? undefined : undefined}
            crypt={previewFile.id.startsWith('crypt:') ? cryptView ?? undefined : undefined}
            onClose={() => setPreviewFile(null)}
            onDownload={() => download([previewFile])}
          />
        ) : previewFile && (
          <VideoPreview
            key={previewFile.id}
            file={previewFile}
            repository={previewFile.id.startsWith('repository:') ? repositoryView ?? undefined : undefined}
            crypt={previewFile.id.startsWith('crypt:') ? cryptView ?? undefined : undefined}
            onQueued={(job) => setJobs((previous) => [job, ...previous].slice(0, 200))}
            onClose={() => setPreviewFile(null)}
            onDownload={() => download([previewFile])}
            alert={alert}
          />
        )}
        {repositoryFolder && (
          <RepositoryWorkspace
            key={`${repositoryFolder.file.id}/${!!repositoryFolder.creating}`}
            folder={repositoryFolder.file}
            probe={repositoryFolder.probe}
            creating={repositoryFolder.creating}
            initialView={repositoryFolder.initialView}
            initialDirectory={repositoryFolder.initialDirectory}
            active={route === 'files'}
            onClose={closeRepository}
            onChanged={() => setRepositoryEpoch((value) => value + 1)}
            onQueued={(added) =>
              setJobs((previous) => [...(added ?? []), ...previous].slice(0, 200))
            }
            controls={repositoryControls}
            fallbackFocus={repositoryManageFocus}
            onView={acceptRepositoryView}
            onInitialFileLocated={(repositoryId, item) => {
              const target = repositoryFile(repositoryId, item);
              setMobileSelecting(false);
              setSelection(new Set([target.id]));
              alert(`已定位到 ${item.name}`);
            }}
            onBusy={setRepositoryBusy}
            onNotice={alert}
          />
        )}
        {managedRepository && (
          <RepositoryWorkspace
            key={managedRepository.file.id}
            folder={managedRepository.file}
            probe={managedRepository.probe}
            manage
            onClose={() => setManagedRepository(null)}
            onChanged={() => setRepositoryEpoch((value) => value + 1)}
          />
        )}
        {cryptFolder && (
          <CryptWorkspace
            key={cryptFolder.file.id}
            folder={cryptFolder.file}
            probe={cryptFolder.probe}
            initialView={cryptFolder.initialView}
            initialDirectory={cryptFolder.initialDirectory}
            active={route === cryptFolder.route}
            onClose={closeCrypt}
            onChanged={() => setCryptEpoch((value) => value + 1)}
            onUnbound={(folderId) =>
              rememberCryptProbes([{ folderId, known: false, saved: false, options: null }])
            }
            onQueued={(added) =>
              setJobs((previous) => [...(added ?? []), ...previous].slice(0, 200))
            }
            controls={cryptControls}
            fallbackFocus={cryptManageFocus}
            onView={acceptCryptView}
            onInitialFileLocated={(folderId, item) => {
              const target = cryptFile(folderId, item);
              setMobileSelecting(false);
              setSelection(new Set([target.id]));
              alert(`已定位到 ${item.name}`);
            }}
            onBusy={setCryptBusy}
            onNotice={alert}
          />
        )}
        {managedCrypt && (
          <CryptWorkspace
            key={`manage:${managedCrypt.file.id}`}
            folder={managedCrypt.file}
            probe={managedCrypt.probe}
            manage
            onClose={() => setManagedCrypt(null)}
            onChanged={() => setCryptEpoch((value) => value + 1)}
            onUnbound={(folderId) =>
              rememberCryptProbes([{ folderId, known: false, saved: false, options: null }])
            }
          />
        )}
        {route === 'settings' || route === 'account' ? (
          <SettingsView
            account={account}
            run={run}
            onAccount={(value) =>
              setAccount((current) => (current?.id === value.id ? value : current))
            }
            sessionsOpen={route === 'account'}
            onSessionsOpen={() => setRoute('account')}
            onSettingsBack={() => setRoute('settings')}
            onRepositoryForgotten={closeRepository}
            oled={oled}
            onOled={setOled}
            systemDark={systemDark}
            theme={theme}
            onTheme={setTheme}
            hideVendorFolders={hideVendorFolders}
            onHideVendorFolders={(value) => {
              setHideVendorFolders(value);
              savePreference('cmcloud.hideVendorFolders', String(value));
              setSelection(new Set());
            }}
          >
            <AccountView
              embedded
              account={account}
              onConnected={applyAccount}
              quota={quota}
              alert={alert}
            />
          </SettingsView>
        ) : route === 'transfers' ? (
          <section className="workspace transfer-workspace">
            <div className="transfer-toolbar">
              <TransferCategories
                value={jobFilter}
                onChange={(value) => {
                  setJobFilter(value);
                  setJobOffset(0);
                }}
              />
              {isOngoing(jobFilter) && (
                <label className="transfer-errors-only">
                  <input
                    type="checkbox"
                    checked={errorsOnly}
                    onChange={(event) => {
                      setErrorsOnly(event.target.checked);
                      setJobOffset(0);
                    }}
                  />
                  仅显示出错项
                </label>
              )}
              {!isOngoing(jobFilter) && (
                <button
                  className="button quiet transfer-clear"
                  disabled={busy}
                  onClick={() => void run(async () => {
                    await api.clear();
                    await refreshTransferRecords(0);
                  }, '已清除完成记录')}
                >
                  <Trash2 size={16} />
                  <span>清除完成记录</span>
                </button>
              )}
            </div>
            {isOngoing(jobFilter) && (
              <TransferBatchActions
                filter={jobFilter}
                summary={batchSummary}
                busy={busy}
                connected={!!account}
                onAction={(action, direction) =>
                  action === 'cancel' ? setCancelBatch(direction) : runBatch(action, direction)
                }
              />
            )}
            {isOngoing(jobFilter) && (
              <TransferOverview
                overview={native ? overview : overviewFromJobs(jobs)}
                filter={jobFilter}
              />
            )}
            <TransferList
              showEmptyHint={!(errorsOnly && isOngoing(jobFilter))}
              emptyTitle={
                errorsOnly && isOngoing(jobFilter)
                  ? '没有出错的任务'
                  : isOngoing(jobFilter)
                    ? '暂无进行中的任务'
                    : '暂无已完成的任务'
              }
              jobs={jobs.filter((job) =>
                matchesTransfer(job, jobFilter, errorsOnly && isOngoing(jobFilter)),
              )}
              control={(id, action) => void run(() => api.control(id, action))}
              open={(id) => void run(() => api.openCompleted(id))}
              removeRecord={(id) => void run(async () => {
                await api.removeCompleted(id);
                setJobs((previous) => previous.filter((job) => job.id !== id));
                await refreshTransferRecords(jobOffset);
              })}
              busy={busy}
              details={setTransferDetail}
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
        ) : route === 'shares' && !parent.sourceId ? (
          <BrowseSources key={account?.id} accountId={account?.id} smallScreen={smallScreen}
            onOpen={openBrowseSource} onConnect={() => setRoute('account')} alert={alert} />
        ) : (
          <section
            hidden={
              (!!repositoryFolder && !repositoryView && route === 'files') ||
              (!!cryptFolder && !cryptView && route === cryptFolder.route)
            }
            className={'workspace files-workspace' + (selection.size > 0 || (smallScreen && mobileSelecting) ? ' selecting-files' : '')}
            ref={uploadDropRef}
            onContextMenu={(event) => {
              if (
                (!account && !parent.sourceId) ||
                (event.target as HTMLElement).closest(
                  'button, input, label, select, textarea, a, .file-row, .address-bar',
                )
              )
                return;
              event.preventDefault();
              setSelection(new Set());
              selectionAnchor.current = null;
              setFileMenu({ x: event.clientX, y: event.clientY, files: [] });
            }}
            onClick={(event) => {
              if (
                !(event.target as HTMLElement).closest(
                  'button, input, label, select, textarea, a, .file-row, .address-bar',
                )
              ) {
                setSelection(new Set());
                selectionAnchor.current = null;
              }
            }}
          >
            {restoring && !account && !parent.sourceId ? (
              <div className="restoring-session" role="status">
                <Loader2 size={22} className="spin" />
                <span>正在恢复会话…</span>
              </div>
            ) : !account && !parent.sourceId ? (
              <ConnectEmpty
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
                <div className={`file-toolbar${searchExpanded ? ' search-expanded' : ''}`}>
                  <div className={`toolbar-actions ${smallScreen ? 'mobile-file-actions' : ''}`}>
                    {selection.size > 0 || (smallScreen && mobileSelecting) ? (
                      <>
                        {smallScreen && <FileSelectAll
                          checked={visible.length > 0 && visible.every((file) => selection.has(file.id))}
                          partiallySelected={visible.some((file) => selection.has(file.id))}
                          disabled={!visible.length}
                          onChange={(checked) => {
                            const next = checked ? new Set(visible.map((file) => file.id)) : new Set<string>();
                            setSelection(next);
                            if (next.size === 0) setMobileSelecting(false);
                          }}
                        />}
                        <span className="selected-count">已选 {selection.size} 项</span>
                        <button
                          className="button quiet"
                          disabled={busy || selection.size === 0}
                          onClick={() => download(selected)}
                        >
                          <Download size={16} />
                          <span>下载</span>
                        </button>
                        {!readOnly && (
                          <>
                            <button
                              className="button quiet"
                              disabled={busy || selection.size === 0}
                              onClick={() => setPrompt({ kind: 'move', files: selected })}
                            >
                              <FolderInput size={16} />
                              <span>移动</span>
                            </button>
                            <button
                              className="button quiet"
                              disabled={busy || selection.size === 0}
                              onClick={() => setPrompt({ kind: 'copy', files: selected })}
                            >
                              <Copy size={16} />
                              <span>复制</span>
                            </button>
                            <button
                              className="button quiet danger"
                              disabled={busy || selection.size === 0}
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
                          onClick={() => {
                            setSelection(new Set());
                            setMobileSelecting(false);
                          }}
                        >
                          <X size={16} />
                        </button>
                      </>
                    ) : smallScreen ? (
                      <>
                        {mobileSearching ? (
                          <button
                            className="icon-button"
                            aria-label="关闭搜索"
                            onClick={() => {
                              setQuery('');
                              setMobileSearching(false);
                              (document.activeElement as HTMLElement | null)?.blur();
                            }}
                          >
                            <X size={20} />
                          </button>
                        ) : (
                          !readOnly && (
                            <button
                              className="icon-button"
                              aria-label="添加"
                              title="添加"
                              disabled={busy}
                              aria-haspopup="dialog"
                              onClick={(event) => {
                                const bounds = event.currentTarget.getBoundingClientRect();
                                setFileMenu({ x: bounds.left, y: bounds.bottom, files: [] });
                              }}
                            >
                              <Plus size={21} />
                            </button>
                          )
                        )}
                        <button
                          className="icon-button"
                          aria-label="显示选项"
                          title="显示选项"
                          aria-haspopup="dialog"
                          aria-expanded={viewMenuOpen}
                          onClick={() => setViewMenuOpen(true)}
                        >
                          <SlidersHorizontal size={20} />
                        </button>
                        {repositoryView && !mobileSearching && (
                          <>
                            <button
                              ref={repositoryManageFocus}
                              className="icon-button"
                              disabled={busy}
                              aria-label="管理对象库"
                              title="管理对象库"
                              onClick={() => repositoryControls.current?.manage()}
                            >
                              <Settings size={19} />
                            </button>
                            <button
                              className="icon-button"
                              disabled={busy}
                              aria-label="退出对象库"
                              title="退出对象库"
                              onClick={closeRepository}
                            >
                              <LogOut size={19} />
                            </button>
                          </>
                        )}
                        {cryptView && !mobileSearching && (
                          <>
                            <button
                              ref={cryptManageFocus}
                              className="icon-button"
                              disabled={busy}
                              aria-label="加密文件夹设置"
                              title="加密文件夹设置"
                              onClick={() => cryptControls.current?.manage()}
                            >
                              <Settings size={19} />
                            </button>
                            <button
                              className="icon-button"
                              disabled={busy}
                              aria-label="退出加密文件夹"
                              title="退出加密文件夹"
                              onClick={closeCrypt}
                            >
                              <LogOut size={19} />
                            </button>
                          </>
                        )}
                      </>
                    ) : (
                      <>
                        {!readOnly && (
                          <button className="button quiet" disabled={busy} onClick={upload}>
                            <Upload size={17} />
                            上传文件
                          </button>
                        )}
                        {!readOnly && (
                          <button
                            className="button quiet"
                            disabled={busy}
                            onClick={() => setPrompt({ kind: 'mkdir', files: [] })}
                          >
                            <FolderPlus size={17} />
                            新建文件夹
                          </button>
                        )}
                        {!readOnly && account?.space === 'personal_new' && !repositoryFolder && !cryptFolder && (
                          <button
                            className="button quiet"
                            disabled={busy}
                            onClick={() => setCreatingRepository(true)}
                          >
                            <HardDrive size={17} />
                            新建对象库
                          </button>
                        )}
                        {!readOnly && !repositoryFolder && !cryptFolder && (
                          <button
                            className="button quiet"
                            disabled={busy}
                            onClick={() => setCreatingCryptFolder(true)}
                          >
                            <ShieldCheck size={17} />
                            新建加密文件夹
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
                        {repositoryView && (
                          <>
                            <button
                              ref={repositoryManageFocus}
                              className="button quiet"
                              disabled={busy}
                              onClick={() => repositoryControls.current?.manage()}
                            >
                              <Settings size={16} />
                              <span>管理对象库</span>
                            </button>
                            <button className="button quiet" disabled={busy} onClick={closeRepository}>
                              <LogOut size={16} />
                              <span>退出对象库</span>
                            </button>
                          </>
                        )}
                        {cryptView && (
                          <>
                            <button
                              ref={cryptManageFocus}
                              className="button quiet"
                              disabled={busy}
                              onClick={() => cryptControls.current?.manage()}
                            >
                              <Settings size={16} />
                              <span>加密文件夹设置</span>
                            </button>
                            <button className="button quiet" disabled={busy} onClick={closeCrypt}>
                              <LogOut size={16} />
                              <span>退出加密文件夹</span>
                            </button>
                          </>
                        )}
                      </>
                    )}
                  </div>
                  <div className="file-search">
                    <div className="search search-field">
                      {searchExpanded ? (
                        <button
                          className="icon-button search-back"
                          aria-label="关闭搜索"
                          title="关闭搜索"
                          onClick={closeExpandedSearch}
                        >
                          <ArrowLeft size={17} />
                        </button>
                      ) : (
                        <Search size={17} />
                      )}
                      <input
                        ref={searchInput}
                        value={query}
                        onFocus={() => {
                          if (smallScreen && !searchExpanded) setMobileSearching(true);
                        }}
                        onChange={(e) => setQuery(e.target.value)}
                        onKeyDown={(event) => {
                          if (searchExpanded && event.key === 'Escape') {
                            event.preventDefault();
                            event.stopPropagation();
                            closeExpandedSearch();
                          }
                        }}
                        placeholder="搜索"
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
                    </div>
                    <button
                      ref={searchButton}
                      className={`search search-trigger${query ? ' has-query' : ''}`}
                      aria-label="打开搜索"
                      title="搜索"
                      onClick={() => setSearchExpanded(true)}
                    >
                      <Search size={17} />
                    </button>
                  </div>
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
                  <FileBreadcrumbs
                    addressRef={addressBarRef}
                    trail={browseTrail}
                    rootIcon={
                      repositoryView ? (
                        <AxuanStorMark />
                      ) : cryptView ? (
                        <OpenListCryptMark />
                      ) : route === 'shares' ? (
                        <Users size={16} />
                      ) : (
                        <Cloud size={16} />
                      )
                    }
                    onNavigate={goTo}
                    onBack={
                      repositoryView ? closeRepository : cryptView ? closeCrypt : undefined
                    }
                    backLabel="返回上级目录"
                    pathValue={currentDisplayPath}
                    pathPlaceholder={
                      repositoryView
                        ? '输入路径，如 /objects/x3client-hotfix:1.7.1300/Total'
                        : cryptView
                          ? '输入加密文件夹内路径，如 /私密备份:文档/合同.pdf'
                          : '输入路径，如 /objects 或 /私密备份:文档'
                    }
                    disabled={busy}
                    onCopyPath={() => copyPath(currentDisplayPath, '当前路径已复制')}
                    onSubmitPath={navigateToPath}
                  />
                  {!smallScreen && (
                    <div className="view-options">
                      {readOnly && <span className="space-tag">只读</span>}
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
                      <label>
                        <select aria-label="显示方式" value={view}
                          onChange={(event) => setView(event.target.value as FileView)}>
                          {(Object.keys(fileViewLabels) as FileView[]).map((mode) => (
                            <option key={mode} value={mode}>{fileViewLabels[mode]}</option>
                          ))}
                        </select>
                      </label>
                    </div>
                  )}
                </div>
                {cryptView && cryptView.unrecognizedCount > 0 && (
                  <div className="crypt-unrecognized-banner" role="status">
                    <span>
                      <Info size={15} />
                      当前目录中有 {cryptView.unrecognizedCount} 个项目无法用当前口令或文件名模式解密（已忽略）
                    </span>
                    <button
                      type="button"
                      className="button quiet"
                      disabled={busy}
                      onClick={() => cryptControls.current?.manage()}
                    >
                      调整口令或参数
                    </button>
                  </div>
                )}
                <div className="files-area">
                  <div className={`file-table ${grid ? 'icon-view ' + view : 'list-view'}`}>
                    {!smallScreen && <FileTableHead
                      sort={sort}
                      onSort={setSort}
                      allSelected={visible.length > 0 && visible.every((file) => selection.has(file.id))}
                      onSelectAll={(checked) => setSelection(checked ? new Set(visible.map((file) => file.id)) : new Set())}
                    />}
                    <div
                      className={`file-scroll ${emptySearch ? 'empty-search-results' : ''}`}
                      ref={scrollRef}
                      {...dragSelection.handlers}
                    >
                      {(pullRefresh.distance > 0 || pullRefresh.refreshing) && (
                        <div
                          className="pull-refresh-indicator"
                          role="status"
                          style={{ height: pullRefresh.refreshing ? 44 : pullRefresh.distance }}
                        >
                          <RefreshCw size={18} className={pullRefresh.refreshing ? 'spin' : ''} />
                          <span>
                            {pullRefresh.refreshing
                              ? '正在刷新'
                              : pullRefresh.distance >= 56
                                ? '松开刷新'
                                : '下拉刷新'}
                          </span>
                        </div>
                      )}
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
                                ? '此目录中暂无可显示的文件。'
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
                          {!smallScreen && (
                            <div className="file-touch-selection-surface" aria-hidden="true" />
                          )}
                          {virtual.getVirtualItems().flatMap((row) =>
                            visible
                              .slice(row.index * columns, (row.index + 1) * columns)
                              .map((file, column) => {
                                return (
                                  <div
                                    key={file.id}
                                    tabIndex={0}
                                    aria-label={file.name}
                                    onPointerDown={(event) => mobileLongPress.start(file.id, event)}
                                    onPointerMove={mobileLongPress.move}
                                    onPointerUp={mobileLongPress.cancel}
                                    onPointerCancel={mobileLongPress.cancel}
                                    onClick={(event) => {
                                      if (mobileLongPress.consumeClick(file.id)) {
                                        event.preventDefault();
                                        return;
                                      }
                                      if (
                                        (event.target as HTMLElement).closest(
                                          '.checkbox-wrap, .row-more',
                                        )
                                      )
                                        return;
                                      if (event.detail > 1) return;
                                      if (smallScreen || !compact) {
                                        if (mobileSelecting || selection.size > 0) toggle(file);
                                        else openFile(file);
                                      } else if (event.shiftKey && selectionAnchor.current) {
                                        const start = visible.findIndex(
                                          (item) => item.id === selectionAnchor.current,
                                        );
                                        const end = visible.findIndex(
                                          (item) => item.id === file.id,
                                        );
                                        const ids =
                                          start < 0
                                            ? [file.id]
                                            : visible
                                                .slice(
                                                  Math.min(start, end),
                                                  Math.max(start, end) + 1,
                                                )
                                                .map((item) => item.id);
                                        setSelection(
                                          new Set(
                                            event.ctrlKey || event.metaKey
                                              ? [...selection, ...ids]
                                              : ids,
                                          ),
                                        );
                                      } else {
                                        if (event.ctrlKey || event.metaKey) toggle(file);
                                        else setSelection(new Set([file.id]));
                                        selectionAnchor.current = file.id;
                                      }
                                    }}
                                    className={`file-row ${selection.has(file.id) ? 'selected' : ''} ${compact ? 'compact' : ''} ${grid ? 'file-card' : ''}`}
                                    style={{
                                      height: rowHeight,
                                      transform: `translateY(${row.start}px)`,
                                      ...(grid
                                        ? {
                                            left: `${(column * 100) / columns}%`,
                                            width: `${100 / columns}%`,
                                            right: 'auto',
                                          }
                                        : {}),
                                    }}
                                    onContextMenu={(event) => {
                                      event.preventDefault();
                                      if (smallScreen) {
                                        mobileLongPress.context(file.id);
                                        return;
                                      }
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
                                        event.key === 'Enter' &&
                                        !(event.target as HTMLElement).closest(
                                          '.checkbox-wrap, .row-more',
                                        )
                                      ) {
                                        event.preventDefault();
                                        openFile(file);
                                      }
                                    }}
                                    onDoubleClick={(event) => {
                                      if (
                                        compact &&
                                        !smallScreen &&
                                        !(event.target as HTMLElement).closest(
                                          '.checkbox-wrap, .row-more',
                                        )
                                      )
                                        openFile(file);
                                    }}
                                  >
                                    <FileRowContents
                                      file={file}
                                      view={view}
                                      smallScreen={smallScreen}
                                      repository={!file.sourceId && repositoryProbes[file.id]?.state === 'repository'}
                                      encryptedFolder={!repositoryView && !cryptView && !!cryptProbes[file.id]?.known}
                                      selected={selection.has(file.id)}
                                      onToggle={() => toggle(file)}
                                      action={
                                        <FileRowMenuButton file={file} disabled={busy} mobile={smallScreen} onOpen={openFileMenu} />
                                      }
                                    />
                                  </div>
                                );
                              }),
                          )}
                          {dragSelection.box && (
                            <div className="file-selection-box" style={dragSelection.box} />
                          )}
                        </div>
                      )}
                      {!recursiveSearch.state && hasMore && (
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
                      {query.trim() &&
                        (recursiveSearch.state || !hasMore || visible.length === 0) && (
                          <div className="recursive-search">
                            {recursiveSearch.state && (
                              <p role="status">
                                {recursiveSearch.state.running ? '正在递归搜索' : '递归搜索已结束'}{' '}
                                · 已检查 {recursiveSearch.state.scanned.toLocaleString()} 项 · 找到{' '}
                                {recursiveSearch.state.files.length.toLocaleString()} 项
                              </p>
                            )}
                            {recursiveSearch.state?.error && (
                              <p role="alert">{recursiveSearch.state.error}</p>
                            )}
                            <button
                              className="button quiet"
                              onClick={() => {
                                if (recursiveSearch.state?.running) recursiveSearch.cancel();
                                else {
                                  setSelection(new Set());
                                  void recursiveSearch.start(
                                    parent,
                                    (file, atRoot) =>
                                      !!repositoryView ||
                                      !!cryptView ||
                                      !hideVendorFolders ||
                                      !isVendorFolder(file, atRoot && trail.length === 1),
                                  );
                                }
                              }}
                            >
                              {recursiveSearch.state?.running ? (
                                <X size={16} />
                              ) : (
                                <Search size={16} />
                              )}
                              {recursiveSearch.state?.running ? '停止搜索' : '递归搜索当前文件夹'}
                            </button>
                            {recursiveSearch.state && (
                              <button className="button quiet" onClick={recursiveSearch.clear}>
                                返回当前文件夹
                              </button>
                            )}
                          </div>
                        )}
                      <footer className="file-footer">
                        <span>
                          已加载 {displayedFiles.length.toLocaleString()} 项
                          {!recursiveSearch.state && hasMore ? ' · 还有更多文件' : ''}
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
                </div>
              </>
            )}
            {dropCount > 0 && (
              <div className="upload-drop-overlay" role="status">
                <Upload size={36} />
                <strong>松开上传到「{trail.length === 1 ? '全部文件' : parent.name}」</strong>
                <span>{dropCount.toLocaleString()} 个文件或文件夹 · 保留文件夹层级</span>
              </div>
            )}
          </section>
        )}
      </div>

      {cancelBatch && (
        <BatchCancelDialog
          direction={cancelBatch}
          count={batchCounts(batchSummary, cancelBatch).cancel}
          busy={busy}
          onClose={() => setCancelBatch(null)}
          onConfirm={() => {
            const direction = cancelBatch;
            setCancelBatch(null);
            runBatch('cancel', direction);
          }}
        />
      )}
      {viewMenuOpen && (
        <FileViewOptions
          view={view}
          setView={setView}
          sort={sort}
          setSort={setSort}
          filter={filter}
          setFilter={setFilter}
          onClose={() => setViewMenuOpen(false)}
        />
      )}
      {smallScreen && (
        <nav
          className="mobile-nav"
          aria-label="主导航"
          style={
            {
              '--nav-index': Math.max(
                0,
                mobileNav.findIndex(
                  (item) => item.route === (route === 'account' ? 'settings' : route),
                ),
              ),
            } as CSSProperties
          }
        >
          {mobileNav.map(({ route: item, label, icon: Icon }) => (
            <button
              key={item}
              className={
                route === item || (item === 'settings' && route === 'account') ? 'active' : ''
              }
              aria-current={
                route === item || (item === 'settings' && route === 'account') ? 'page' : undefined
              }
              onClick={() => switchRoute(item)}
            >
              <span className="mobile-nav-icon">
                <Icon size={21} />
                {item === 'transfers' && activeCount > 0 && (
                  <span className="mobile-count">{activeCount > 99 ? '99+' : activeCount}</span>
                )}
              </span>
              <span>{label}</span>
            </button>
          ))}
        </nav>
      )}
      {fileMenu && (
        <FileContextMenu x={fileMenu.x} y={fileMenu.y} items={menuItems} onClose={closeFileMenu}
          mobile={smallScreen}
          title={fileMenu.files.length ? '文件操作' : '目录操作'}
          target={fileMenu.files.length === 1 ? fileMenu.files[0].name
            : fileMenu.files.length > 1 ? fileMenu.files.length + ' 个项目' : parent.name} />
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
      {creatingRepository && (
        <RepositoryCreateDialog
          parent={parent}
          onClose={() => setCreatingRepository(false)}
          onCreated={(view, name) => {
            setCreatingRepository(false);
            setSelection(new Set());
            setRepositoryEpoch((v) => v + 1);
            setRepositoryFolder({
              file: {
                id: view.folderId,
                name,
                path: `${parent.path}/${name}`,
                isDir: true,
                size: 0,
                modifiedAt: new Date().toISOString(),
              },
              probe: {
                folderId: view.folderId,
                repositoryId: view.repositoryId,
                kind: view.encrypted ? 'encrypted' : 'plain',
                state: 'repository',
                saved: view.saved,
              },
              initialView: view,
              cloudPath: formatCloudPath([...trail.slice(1), { name }]),
            });
            refresh(true);
          }}
        />
      )}
      {creatingCryptFolder && (
        <CryptCreateDialog
          parent={parent}
          onClose={() => setCreatingCryptFolder(false)}
          onCreated={(view, name) => {
            setCreatingCryptFolder(false);
            setSelection(new Set());
            setCryptEpoch((v) => v + 1);
            rememberCryptProbes([
              {
                folderId: view.folderId,
                known: true,
                saved: view.saved,
                options: view.options,
              },
            ]);
            setCryptFolder({
              file: {
                id: view.folderId,
                name,
                path: `${parent.path}/${name}`,
                isDir: true,
                size: 0,
                modifiedAt: new Date().toISOString(),
              },
              probe: {
                folderId: view.folderId,
                known: true,
                saved: view.saved,
                options: view.options,
              },
              initialView: view,
              cloudPath: formatCloudPath([...trail.slice(1), { name }]),
              route,
            });
            refresh(true);
          }}
        />
      )}
      {prompt && (
        <OperationDialog
          prompt={prompt}
          parent={parent}
          root={browseTrail[0]}
          operations={activeOps}
          sourceRepository={
            account?.space === 'personal_new' && repositoryView && !repositoryView.encrypted
              ? repositoryView
              : undefined
          }
          cloudRoot={
            account?.space === 'personal_new' && !cryptView ? personalTrail[0] : undefined
          }
          knownProbes={account?.space === 'personal_new' ? repositoryProbes : undefined}
          onClose={() => setPrompt(null)}
          onRemoved={(ids) => {
            if (repositoryRef.current || cryptRef.current) {
              const removed = new Set(ids);
              setFiles((previous) => previous.filter((file) => !removed.has(file.id)));
              return;
            }
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
            if (repositoryRef.current || cryptRef.current) {
              setPrompt(null);
              setSelection(new Set());
              void refresh(true);
              return;
            }
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
  native,
  onConnect,
  onImport,
}: {
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
        <h2>连接移动云盘</h2>
        <p>登录账号，浏览个人云中的文件。</p>
        <div className="connect-actions">
          <button className="button primary" onClick={onConnect}>
            <Cloud size={18} />
            连接移动云盘
            <ArrowRight size={17} />
          </button>
          {!mobile && (
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
    </div>
  );
}

function TransferList({
  emptyTitle,
  showEmptyHint,
  jobs,
  control,
  open,
  removeRecord,
  busy,
  details,
}: {
  emptyTitle: string;
  showEmptyHint: boolean;
  jobs: TransferJob[];
  control: (id: string, action: string) => void;
  open: (id: string) => void;
  removeRecord: (id: string) => void;
  busy: boolean;
  details: (job: TransferJob) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const virtual = useVirtualizer({
    count: jobs.length,
    getScrollElement: () => ref.current,
    estimateSize: (index) => jobs[index]?.status === 'completed' ? 96 : 120,
    getItemKey: (index) => jobs[index].id,
    overscan: 8,
  });
  if (!jobs.length)
    return (
      <div className="transfer-empty">
        <ArrowDownToLine size={45} strokeWidth={1.2} />
        <h2>{emptyTitle}</h2>
        {showEmptyHint && <p>在文件列表中上传或下载，任务会显示在这里。</p>}
      </div>
    );
  return (
    <div className="transfer-scroll" ref={ref}>
      <div style={{ height: virtual.getTotalSize(), position: 'relative' }}>
        {virtual.getVirtualItems().map((item) => {
          const job = jobs[item.index];
          const progress = job.size > 0 ? Math.min((job.completed / job.size) * 100, 100) : 0;
          const active = [
            'running',
            'hashing',
            'encrypting',
            'decrypting',
            'publishing',
            'downloading',
            'uploading',
            'queued',
          ].includes(job.status);
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
                  {job.status !== 'completed' && (job.status === 'hashing' || job.error) && (
                    <span>{job.status === 'hashing' ? '分块读取，计算文件校验值' : job.error}</span>
                  )}
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
                ) : job.status === 'completed' ? (
                  <>
                    {job.direction === 'download' && (
                      <button className="button quiet transfer-open" aria-label={`打开 ${job.name}`}
                        title="打开已下载文件" disabled={busy} onClick={() => open(job.id)}>
                        <ExternalLink size={17} />
                        打开
                      </button>
                    )}
                    <button className="icon-button" aria-label={`删除 ${job.name} 的记录`}
                      title="删除记录" disabled={busy} onClick={() => removeRecord(job.id)}>
                      <X size={18} />
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

export function VideoPreview({
  file,
  repository,
  crypt,
  onQueued,
  onClose,
  onDownload,
  alert,
}: {
  file: CloudFile;
  repository?: RepositoryView;
  crypt?: CryptView;
  onQueued?: (job: TransferJob) => void;
  onClose: () => void;
  onDownload: () => void;
  alert: (message: string, error?: boolean) => void;
}) {
  const [sources, setSources] = useState<{ url: string; label: string }[]>([]);
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [localJob, setLocalJob] = useState<string | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [showFullscreenControl, setShowFullscreenControl] = useState(true);
  const video = useRef<HTMLVideoElement>(null);
  const fullscreenTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const revealFullscreenControl = useCallback(() => {
    clearTimeout(fullscreenTimer.current);
    setShowFullscreenControl(true);
    if (video.current && !video.current.paused) {
      fullscreenTimer.current = setTimeout(() => setShowFullscreenControl(false), 3000);
    }
  }, []);
  const immersiveRequest = useRef(Promise.resolve());
  useEffect(() => {
    if (!fullscreen) return;
    revealFullscreenControl();
    immersiveRequest.current = immersiveRequest.current.then(() => setVideoImmersive(true)).catch(() => {});
    const back = (event: Event) => {
      event.preventDefault();
      setFullscreen(false);
    };
    window.addEventListener('cmcloud-preview-back', back);
    return () => {
      clearTimeout(fullscreenTimer.current);
      window.removeEventListener('cmcloud-preview-back', back);
      immersiveRequest.current = immersiveRequest.current.then(() => setVideoImmersive(false)).catch(() => {});
    };
  }, [fullscreen, revealFullscreenControl]);
  const requests = useRef(new Map<string, Promise<RepositoryPreviewStart>>());
  const cryptRequests = useRef(new Map<string, Promise<CryptPreviewStart>>());
  const cleanup = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const [streamId, setStreamId] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    setLoading(true);
    setError('');
    setSource('');
    setLocalJob(null);
    setStreamId(null);
    const key = repository
      ? `repo:${repository.handle}:${repository.revision}:${file.path}`
      : crypt
        ? `crypt:${crypt.handle}:${file.path}`
        : '';
    let pending: Promise<RepositoryPreviewStart> | undefined;
    let cryptPending: Promise<CryptPreviewStart> | undefined;
    if (repository) {
      const timer = cleanup.current.get(key);
      if (timer !== undefined) {
        clearTimeout(timer);
        cleanup.current.delete(key);
      }
      pending = requests.current.get(key);
      if (!pending) {
        pending = repositoryApi.startPreview(repository.handle, repository.revision, file.path);
        requests.current.set(key, pending);
      }
    } else if (crypt) {
      const timer = cleanup.current.get(key);
      if (timer !== undefined) {
        clearTimeout(timer);
        cleanup.current.delete(key);
      }
      cryptPending = cryptRequests.current.get(key);
      if (!cryptPending) {
        cryptPending = cryptApi.startPreview(crypt.handle, file.path);
        cryptRequests.current.set(key, cryptPending);
      }
    }
    void (async () => {
      try {
        let values: { url: string; label: string }[] = [];
        if (cryptPending) {
          const started = await cryptPending;
          if (disposed) return;
          if (started.url) {
            setStreamId(started.streamId ?? null);
            values = [{ url: started.url, label: '本机流式解密播放' }];
          }
        } else if (pending) {
          const started = await pending;
          if (disposed) return;
          if (started.url) {
            setStreamId(started.streamId ?? null);
            values = [{ url: started.url, label: started.streamId ? '本机流式播放' : '原始文件' }];
          }
          else if (started.job) {
            const job = started.job;
            setLocalJob(job.id);
            onQueued?.(normalizeJob(job));
            while (!disposed) {
              const ready = await repositoryApi.previewReady(job.id);
              if (disposed) return;
              if (ready.path) {
                values = [{ url: repositoryPreviewUrl(ready.path), label: '本机解密预览' }];
                break;
              }
              if (ready.status === 'paused')
                throw new Error('预览任务已暂停。请在传输列表继续任务后重新打开。');
              await new Promise((resolve) => setTimeout(resolve, 350));
            }
          }
        } else {
          if (fileKind(file) === '视频') values = previewSources(await api.preview(file).catch(() => null));
          if (!values.length) values = [{ url: await api.link(file), label: '原始文件' }];
        }
        if (!disposed) {
          if (!values.length) throw new Error('没有可用的预览源');
          setSources(values);
          setSource(values[0].url);
        }
      } catch (reason) {
        if (!disposed)
          setError(
            pending || cryptPending ? errorText(reason) : '此文件暂时无法预览，请下载后打开。',
          );
      } finally {
        if (!disposed) setLoading(false);
      }
    })();
    return () => {
      disposed = true;
      if (cryptPending) {
        const release = cryptPending;
        cleanup.current.set(
          key,
          setTimeout(() => {
            cleanup.current.delete(key);
            cryptRequests.current.delete(key);
            void release
              .then(
                (started) => (started.streamId ? cryptApi.releasePreview(started.streamId) : undefined),
                () => undefined,
              )
              .catch((reason) => alert(errorText(reason), true));
          }, 0),
        );
      } else if (pending) {
        const release = pending;
        cleanup.current.set(key, setTimeout(() => {
          cleanup.current.delete(key);
          requests.current.delete(key);
          void release.then((started) => {
            const id = started.streamId ?? started.job?.id;
            return id ? repositoryApi.releasePreview(id) : undefined;
          }, () => undefined).catch((reason) => alert(errorText(reason), true));
        }, 0));
      }
    };
  }, [file.id, repository?.handle, repository?.revision, crypt?.handle]);
  return (
    <PreviewWorkspace name={file.name} detail={fileKind(file) + ' · ' + formatBytes(file.size)}
      onClose={onClose} onDownload={onDownload}>
      {loading ? (
        <p className="preview-state" role="status">
          <Loader2 size={18} className="spin" />
          {localJob ? '正在下载并解密，可在传输列表查看进度…' : '正在获取预览源…'}
        </p>
      ) : error ? (
        <div className="preview-state" role="alert">
          <Info size={17} />
          {error}
        </div>
      ) : (
        <div className="preview-media">
          {fileKind(file) === '图片' ? (
            <PreviewZoom className="preview-media-frame" media key={source}>
              <img
                className="file-preview-image"
                src={source}
                alt={file.name}
                onError={() => setError('此图片无法在设备中显示，请下载后打开。')}
              />
            </PreviewZoom>
          ) : fileKind(file) === '音频' ? (
            <audio
              src={source}
              controls
              preload="metadata"
              onError={() => setError(streamId
                ? '内置播放器无法播放此音频。可尝试外部播放器，或下载后打开。'
                : '此音频无法在设备中播放，请下载后打开。')}
            />
          ) : (
            <div className={'preview-video' + (android ? ' preview-video-android' : '') +
              (fullscreen ? ' preview-video-fullscreen' : '')}
              onPointerDownCapture={fullscreen ? revealFullscreenControl : undefined}
              onPointerMove={fullscreen ? (event) => {
                if (event.pointerType === 'mouse') revealFullscreenControl();
              } : undefined}
              onKeyDownCapture={fullscreen ? revealFullscreenControl : undefined}>
              <PreviewZoom className="preview-media-frame" media key={source}>
                <video
                  ref={video}
                  src={source}
                  controls
                  controlsList={android ? 'nofullscreen' : undefined}
                  playsInline
                  preload="metadata"
                  onPlay={fullscreen ? revealFullscreenControl : undefined}
                  onPause={fullscreen ? revealFullscreenControl : undefined}
                  onError={() => {
                    setFullscreen(false);
                    setError(streamId
                      ? '内置播放器无法播放此视频。可尝试外部播放器，或下载后打开。'
                      : '当前播放源无法在设备内播放。可打开外部预览，或下载后使用本机播放器。');
                  }}
                />
              </PreviewZoom>
              {android && (
                <button type="button" className="preview-video-fullscreen-toggle icon-button"
                  data-hidden={fullscreen && !showFullscreenControl || undefined}
                  aria-label={fullscreen ? '退出全屏' : '全屏播放'}
                  onClick={() => setFullscreen((value) => !value)}>
                  {fullscreen ? <Minimize size={20} /> : <Maximize size={20} />}
                </button>
              )}
            </div>
          )}
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
        </div>
      )}
      {source && (
        <div className="document-controls preview-media-actions">
          <button
            className="button secondary"
            onClick={() =>
              void (streamId
                ? crypt
                  ? cryptApi.openMediaPlayer(streamId)
                  : repositoryApi.openMediaPlayer(streamId)
                : localJob
                  ? api.openCompleted(localJob)
                  : openUrl(source)
              ).catch((e) => alert(errorText(e), true))
            }
          >
            <ExternalLink size={16} />
            {streamId ? '外部播放器' : '外部预览'}
          </button>
          <button
            className="button secondary"
            disabled={!!localJob || !!streamId}
            title={localJob || streamId ? '解密预览仅供本机使用，关闭后回收' : ''}
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
    </PreviewWorkspace>
  );
}

export function SettingsView({
  children,
  sessionsOpen,
  onSessionsOpen,
  onSettingsBack,
  oled,
  onOled,
  systemDark,
  account,
  run,
  onAccount,
  theme,
  onTheme,
  hideVendorFolders,
  onHideVendorFolders,
  onRepositoryForgotten,
}: {
  account: AccountSummary | null;
  run: (action: () => Promise<unknown>, success?: string) => Promise<void>;
  onAccount: (account: AccountSummary) => void;
  systemDark: boolean;
  oled: boolean;
  onOled: (value: boolean) => void;
  theme: Theme;
  onTheme: (theme: Theme) => void;
  hideVendorFolders: boolean;
  onHideVendorFolders: (value: boolean) => void;
  onRepositoryForgotten?: () => void;
  children: ReactNode;
  sessionsOpen: boolean;
  onSessionsOpen: () => void;
  onSettingsBack: () => void;
}) {
  const [concurrency, setConcurrency] = useState(4);
  const [saved, setSaved] = useState(false);
  const [baseline, setBaseline] = useState({
    concurrency: 4,
    partSize: 0,
    large: false,
  });
  const settingsScope = useRef(account?.id);
  settingsScope.current = account?.id;
  const [saving, setSaving] = useState(false);
  const [optionsError, setOptionsError] = useState('');
  const [retry, setRetry] = useState(0);
  const savingRef = useRef(false);
  const [partSize, setPartSize] = useState(0);
  const [large, setLarge] = useState(false);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    setSaved(false);
    setOptionsError('');
    void api
      .options()
      .then((options) => {
        if (disposed) return;
        setConcurrency(options.concurrency);
        setPartSize(options.customUploadPartSize);
        setLarge(options.useLargeThumbnail);
        setBaseline({
          concurrency: options.concurrency,
          partSize: options.customUploadPartSize,
          large: options.useLargeThumbnail,
        });
        setSaved(true);
      })
      .catch((error) => {
        if (!disposed) setOptionsError(errorText(error));
      });
    return () => {
      disposed = true;
    };
  }, [account?.id, retry]);
  const changed = {
    concurrency: saved && concurrency !== baseline.concurrency,
    partSize: saved && !!account && partSize !== baseline.partSize,
    large: saved && !!account && large !== baseline.large,
  };
  const hasChanges = Object.values(changed).some(Boolean);
  return (
    <section className="workspace settings-workspace">
      {sessionsOpen ? (
        <>
          <div className="settings-backbar">
            <button className="button quiet" onClick={onSettingsBack}>
              <ArrowLeft size={18} />
              返回设置
            </button>
          </div>
          {children}
        </>
      ) : (
        <div className="settings-content">
          <button
            className="settings-account-entry"
            aria-label="账号管理"
            disabled={saving}
            onClick={onSessionsOpen}
          >
            <span className="settings-account-icon">
              <UserRound size={24} strokeWidth={1.6} />
            </span>
            <span>
              <strong>{account?.phone || account?.account || '连接移动云盘'}</strong>
              <small>账号管理</small>
            </span>
            <ChevronRight size={18} />
          </button>
          <div className="settings-panel">
            <section className="settings-group" aria-labelledby="appearance-settings">
              <h2 id="appearance-settings">外观</h2>
              <div className="setting-row">
                <div>
                  <h3>外观模式</h3>
                </div>
                <select
                  aria-label="外观模式"
                  value={theme}
                  onChange={(event) => onTheme(event.target.value as Theme)}
                >
                  <option value="system">跟随系统</option>
                  <option value="light">浅色模式</option>
                  <option value="dark">深色模式</option>
                </select>
              </div>
              {(theme === 'dark' || (theme === 'system' && systemDark)) && (
                <Toggle
                  title="OLED 纯黑"
                  description="使用纯黑背景。"
                  value={oled}
                  onChange={onOled}
                />
              )}
            </section>
            <section className="settings-group" aria-labelledby="transfer-settings">
              <h2 id="transfer-settings">传输</h2>
              {android && (
                <div className="setting-row">
                  <h3>允许后台高耗电</h3>
                  <button
                    className="button secondary"
                    onClick={() => void run(() => api.requestTransferProtection())}
                  >
                    设置后台保护
                  </button>
                </div>
              )}
              <div className={`setting-row ${changed.concurrency ? 'is-modified' : ''}`}>
                <div>
                  <h3>同时传输任务</h3>
                  <p>设置同时进行的传输任务数量。</p>
                </div>
                <select
                  disabled={!saved}
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
              <div className={`setting-row ${changed.partSize ? 'is-modified' : ''}`}>
                <div>
                  <h3>上传分片大小</h3>
                  <p>自动选择，或指定每个上传分片的大小。</p>
                </div>
                <select
                  aria-label="上传分片大小"
                  disabled={!saved || !account}
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
            </section>
            <section className="settings-group" aria-labelledby="browser-settings">
              <h2 id="browser-settings">文件浏览</h2>
              <Toggle
                title="隐藏厂商文件夹"
                value={hideVendorFolders}
                onChange={onHideVendorFolders}
              />
              <Toggle
                title="使用大尺寸缩略图"
                description="优先加载更清晰的图片预览，会增加网络用量。"
                modified={changed.large}
                disabled={!saved || !account}
                value={large}
                onChange={setLarge}
              />
            </section>
            <RepositorySettings onForgotten={onRepositoryForgotten} />
          </div>
          <div className="settings-about">
            <svg viewBox="0 0 36 36" width="30" height="30" aria-hidden="true">
              <defs>
                <linearGradient id="cm-about-grad" x1="3" y1="2" x2="33" y2="34" gradientUnits="userSpaceOnUse">
                  <stop offset="0" stopColor="#1e40af" />
                  <stop offset="0.52" stopColor="#155eef" />
                  <stop offset="1" stopColor="#0ea5e9" />
                </linearGradient>
              </defs>
              <rect width="36" height="36" rx="9" fill="url(#cm-about-grad)" />
              <path
                d="M17.8 24.8H10.2a5.1 5.1 0 0 1-1.8-9.85 8.1 8.1 0 0 1 15.4-1.4 4.8 4.8 0 0 1 4.1 2.45"
                fill="none"
                stroke="#fff"
                strokeWidth="2.4"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              <path
                d="M12.8 14.3a4.6 4.6 0 0 1 7-1"
                fill="none"
                stroke="#bae6fd"
                strokeOpacity="0.75"
                strokeWidth="1.3"
                strokeLinecap="round"
              />
              <path
                d="M22.8 22v4M20.8 24h4"
                fill="none"
                stroke="#fff"
                strokeWidth="2.05"
                strokeLinecap="round"
              />
              <path
                d="M28.8 18.8v4M26.8 20.8h4"
                fill="none"
                stroke="#7dd3fc"
                strokeWidth="2.05"
                strokeLinecap="round"
              />
            </svg>
            <div>
              <strong>CMCloud++</strong>
              <p>
                <a
                  href="https://github.com/mumingluan/CMCloudPlusPlus"
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(event) => {
                    if (native) {
                      event.preventDefault();
                      void run(async () =>
                        openUrl('https://github.com/mumingluan/CMCloudPlusPlus'),
                      );
                    }
                  }}
                >
                  GitHub 源码
                </a>
              </p>
            </div>
          </div>
          {optionsError && (
            <div className="settings-load-error" role="alert">
              <span>设置读取失败：{optionsError}</span>
              <button className="button quiet" onClick={() => setRetry((value) => value + 1)}>
                重试
              </button>
            </div>
          )}
          <footer className="settings-save">
            <button
              className="button primary"
              disabled={!native || !saved || !hasChanges || saving}
              onClick={() => {
                if (savingRef.current || !saved || !hasChanges) return;
                savingRef.current = true;
                setSaving(true);
                const submitted = { concurrency, partSize, large };
                const scope = account?.id;
                void run(async () => {
                  if (changed.concurrency) {
                    await api.settings(submitted.concurrency);
                    if (settingsScope.current === scope)
                      setBaseline((previous) => ({
                        ...previous,
                        concurrency: submitted.concurrency,
                      }));
                  }
                  if (account && (changed.partSize || changed.large)) {
                    const updated = await api.updateOptions({
                      customUploadPartSize: submitted.partSize,
                      useLargeThumbnail: submitted.large,
                    });
                    if (settingsScope.current === scope) {
                      setBaseline((previous) => ({
                        ...previous,
                        partSize: submitted.partSize,
                        large: submitted.large,
                      }));
                      onAccount(updated);
                    }
                  }
                }, '设置已保存').finally(() => {
                  savingRef.current = false;
                  setSaving(false);
                });
              }}
            >
              {saving ? <Loader2 size={17} className="spin" /> : <Check size={17} />}
              保存设置
            </button>
          </footer>
        </div>
      )}
    </section>
  );
}
function Toggle({
  title,
  description,
  value,
  onChange,
  modified = false,
  disabled = false,
}: {
  title: string;
  description?: string;
  value: boolean;
  onChange: (v: boolean) => void;
  modified?: boolean;
  disabled?: boolean;
}) {
  return (
    <div className={`setting-row ${modified ? 'is-modified' : ''}`}>
      <div>
        <h3>{title}</h3>
        {description && <p>{description}</p>}
      </div>
      <button
        className={`toggle ${value ? 'on' : ''}`}
        role="switch"
        disabled={disabled}
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
  operations = api,
  sourceRepository,
  cloudRoot,
  knownProbes,
}: {
  prompt: Prompt;
  parent: CloudFile;
  root: CloudFile;
  onClose: () => void;
  onDone: (name?: string) => void;
  onRemoved?: (ids: string[]) => void;
  alert: (text: string, error?: boolean) => void;
  operations?: Pick<typeof api, 'list' | 'mkdir' | 'rename' | 'remove' | 'relocate'>;
  sourceRepository?: RepositoryView;
  cloudRoot?: CloudFile;
  knownProbes?: Record<string, RepositoryProbe>;
}) {
  const titleId = useId();
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(prompt.kind === 'rename' ? (prompt.files[0]?.name ?? '') : '');
  const [deleteTargets, setDeleteTargets] = useState(prompt.files);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const canSwitchToCloud =
    ['move', 'copy'].includes(prompt.kind) && !!sourceRepository && !!cloudRoot;
  const [targetScope, setTargetScope] = useState<'current' | 'cloud'>('current');
  const activeBaseRoot = targetScope === 'cloud' && cloudRoot ? cloudRoot : root;
  const [dest, setDest] = useState(root);
  const [dirs, setDirs] = useState<CloudFile[]>([]);
  const [destTrail, setDestTrail] = useState<CloudFile[]>([root]);
  const [dirCursor, setDirCursor] = useState<string | null>(null);
  const [probes, setProbes] = useState<Record<string, RepositoryProbe>>(() => ({
    ...(knownProbes ?? {}),
  }));
  const [enteredRepo, setEnteredRepo] = useState<{
    handle: string;
    revision: string;
    repositoryId: string;
    folderId: string;
    ownedHandle: boolean;
    entryIndex: number;
  } | null>(null);
  const ownedHandlesRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (knownProbes) {
      setProbes((prev) => ({ ...knownProbes, ...prev }));
    }
  }, [knownProbes]);
  useEffect(() => {
    dialog.current?.showModal();
    const owned = ownedHandlesRef.current;
    return () => {
      dialog.current?.close();
      for (const h of owned) {
        void repositoryApi.close(h).catch(() => {});
      }
      owned.clear();
    };
  }, []);
  const [dirLoading, setDirLoading] = useState(false);
  const destinationRequest = useRef(0);
  const pagePending = useRef(false);
  const submitting = useRef(false);
  const browsingCloudTree =
    ['move', 'copy'].includes(prompt.kind) &&
    !enteredRepo &&
    (targetScope === 'cloud' || (!sourceRepository && !!cloudRoot));
  const listOperator = targetScope === 'cloud' ? api : operations;

  useEffect(() => {
    if (!['move', 'copy'].includes(prompt.kind)) return;
    const request = ++destinationRequest.current;
    let disposed = false;
    pagePending.current = true;
    setDirs([]);
    setDirCursor(null);
    setDirLoading(true);
    setError('');
    const loader = enteredRepo
      ? repositoryApi.list(enteredRepo.handle, dest.path).then((v) => ({
          files: v.items.map((item) => repositoryFile(v.repositoryId, item)),
          nextCursor: null,
          revision: v.revision,
        }))
      : listOperator.list(dest, null).then((p) => ({ ...p, revision: undefined }));
    void loader
      .then((page) => {
        if (!disposed && destinationRequest.current === request) {
          const nextDirs = page.files.filter((f) => f.isDir);
          setDirs(nextDirs);
          setDirCursor(page.nextCursor ?? null);
          if (enteredRepo && page.revision) {
            setEnteredRepo((cur) =>
              cur && cur.handle === enteredRepo.handle ? { ...cur, revision: page.revision! } : cur,
            );
          }
          if (!enteredRepo && browsingCloudTree && nextDirs.length > 0) {
            const unknownIds = nextDirs
              .map((d) => d.id)
              .filter((id) => id && !probes[id])
              .slice(0, 32);
            if (unknownIds.length > 0) {
              void repositoryApi
                .probe(unknownIds)
                .then((rows) => {
                  if (disposed) return;
                  setProbes((prev) => {
                    const merged = { ...prev };
                    for (const r of rows) merged[r.folderId] = r;
                    return merged;
                  });
                })
                .catch(() => {});
            }
          }
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
  }, [dest, prompt.kind, listOperator, enteredRepo?.handle]);

  const switchTargetScope = (next: 'current' | 'cloud') => {
    if (next === targetScope || busy) return;
    destinationRequest.current++;
    pagePending.current = false;
    if (enteredRepo?.ownedHandle) {
      ownedHandlesRef.current.delete(enteredRepo.handle);
      void repositoryApi.close(enteredRepo.handle).catch(() => {});
    }
    setEnteredRepo(null);
    const nextRoot = next === 'cloud' && cloudRoot ? cloudRoot : root;
    setTargetScope(next);
    setDirs([]);
    setDirCursor(null);
    setDirLoading(true);
    setDest(nextRoot);
    setDestTrail([nextRoot]);
  };

  const navigateDestination = async (directory: CloudFile, path: CloudFile[]) => {
    if (busy) return;
    if (enteredRepo && path.length - 1 < enteredRepo.entryIndex) {
      if (enteredRepo.ownedHandle) {
        ownedHandlesRef.current.delete(enteredRepo.handle);
        void repositoryApi.close(enteredRepo.handle).catch(() => {});
      }
      setEnteredRepo(null);
    }
    if (!enteredRepo && browsingCloudTree) {
      const probe = probes[directory.id];
      if (probe?.state === 'repository') {
        if (probe.kind !== 'plain' || !probe.repositoryId) {
          setError('加密对象库不支持直接移入');
          return;
        }
        if (sourceRepository && sourceRepository.repositoryId === probe.repositoryId) {
          switchTargetScope('current');
          return;
        }
        const req = ++destinationRequest.current;
        setDirLoading(true);
        setError('');
        try {
          const opened = await repositoryApi.open(directory.id, probe.repositoryId);
          if (destinationRequest.current !== req) {
            void repositoryApi.close(opened.handle).catch(() => {});
            return;
          }
          ownedHandlesRef.current.add(opened.handle);
          const repoRoot = repositoryFile(opened.repositoryId, {
            name: directory.name,
            path: '',
            kind: 'directory',
            size: 0,
            conflict: false,
            protected: false,
          });
          const nextTrail = [...path.slice(0, -1), repoRoot];
          setEnteredRepo({
            handle: opened.handle,
            revision: opened.revision,
            repositoryId: opened.repositoryId,
            folderId: directory.id,
            ownedHandle: true,
            entryIndex: nextTrail.length - 1,
          });
          setDest(repoRoot);
          setDestTrail(nextTrail);
        } catch (e) {
          if (destinationRequest.current === req) {
            setError(errorText(e));
            setDirLoading(false);
          }
        }
        return;
      }
    }
    destinationRequest.current++;
    pagePending.current = false;
    setDirs([]);
    setDirCursor(null);
    setDirLoading(true);
    setDest(directory);
    setDestTrail(path);
  };
  const loadDestinationPage = async () => {
    if (pagePending.current || !dirCursor || enteredRepo) return;
    const request = destinationRequest.current;
    pagePending.current = true;
    setDirLoading(true);
    try {
      const page = await listOperator.list(dest, dirCursor);
      if (destinationRequest.current !== request) return;
      const added = page.files.filter((file) => file.isDir);
      setDirs((previous) => {
        const ids = new Set(previous.map((file) => file.id));
        return [...previous, ...added.filter((file) => !ids.has(file.id))];
      });
      if (browsingCloudTree && added.length > 0) {
        const unknownIds = added
          .map((d) => d.id)
          .filter((id) => id && !probes[id])
          .slice(0, 32);
        if (unknownIds.length > 0) {
          void repositoryApi
            .probe(unknownIds)
            .then((rows) => {
              setProbes((prev) => {
                const merged = { ...prev };
                for (const r of rows) merged[r.folderId] = r;
                return merged;
              });
            })
            .catch(() => {});
        }
      }
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
      const isCopy = prompt.kind === 'copy';
      if (prompt.kind === 'mkdir') await operations.mkdir(parent, name.trim());
      else if (prompt.kind === 'rename') await operations.rename(prompt.files[0], name.trim());
      else if (prompt.kind === 'delete') {
        const result = await operations.remove(deleteTargets);
        const removed = new Set(result.removedIds);
        setDeleteTargets((previous) => previous.filter((file) => !removed.has(file.id)));
        onRemoved?.(result.removedIds);
        if (result.errors.length)
          throw new Error(`部分项目删除失败；已成功的操作保留。\n${result.errors.join('\n')}`);
      } else if (sourceRepository && targetScope === 'cloud' && enteredRepo) {
        await repositoryApi.relocateBetweenRepositories(
          sourceRepository.handle,
          sourceRepository.revision,
          prompt.files.map((f) => f.path),
          enteredRepo.handle,
          enteredRepo.revision,
          dest.path,
          isCopy,
        );
      } else if (sourceRepository && targetScope === 'cloud') {
        await repositoryApi.exportToCloud(
          sourceRepository.handle,
          sourceRepository.revision,
          prompt.files.map((f) => f.path),
          dest,
          isCopy,
        );
      } else if (!sourceRepository && enteredRepo) {
        await repositoryApi.importFromCloud(
          enteredRepo.handle,
          enteredRepo.revision,
          dest.path,
          prompt.files,
          isCopy,
        );
      } else {
        await operations.relocate(prompt.files, dest, isCopy);
      }
      onDone(name.trim());
      alert(prompt.kind === 'delete' ? '所选文件已删除' : '操作已完成');
    } catch (e) {
      setError(errorText(e));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };
  void activeBaseRoot;
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
      aria-labelledby={titleId}
      onCancel={(e) => {
        if (busy) e.preventDefault();
        else onClose();
      }}
    >
      <div className="dialog-heading">
        <h2 id={titleId}>{title}</h2>
        <button className="icon-button" disabled={busy} aria-label="关闭" onClick={onClose}>
          <X size={19} />
        </button>
      </div>
      {prompt.kind === 'delete' ? (
        <div className="delete-content">
          <Trash2 size={30} />
          <p>将删除 {deleteTargets.length} 个项目。</p>
          <small>继续前请确认所选文件。</small>
          <ul>
            {deleteTargets.slice(0, 5).map((file) => (
              <li key={file.id}>{file.name}</li>
            ))}
          </ul>
          {prompt.files.length > 5 && <small>以及另外 {prompt.files.length - 5} 项</small>}
        </div>
      ) : ['move', 'copy'].includes(prompt.kind) ? (
        <>
          {canSwitchToCloud && (
            <div className="destination-scope-tabs" role="tablist" aria-label="目标位置范围">
              <button
                type="button"
                role="tab"
                aria-selected={targetScope === 'current'}
                className={targetScope === 'current' ? 'active' : ''}
                disabled={busy}
                onClick={() => switchTargetScope('current')}
              >
                <AxuanStorMark />
                当前对象库
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={targetScope === 'cloud'}
                className={targetScope === 'cloud' ? 'active' : ''}
                disabled={busy}
                onClick={() => switchTargetScope('cloud')}
              >
                <Cloud size={15} />
                移动云盘目录
              </button>
            </div>
          )}
          <div className="destination-path">
            {destTrail.map((d, i) => (
              <button
                key={i}
                disabled={busy || i === destTrail.length - 1}
                onClick={() => void navigateDestination(d, destTrail.slice(0, i + 1))}
              >
                {enteredRepo && i === enteredRepo.entryIndex && <AxuanStorMark />}
                {d.name}
                <ChevronRight size={13} />
              </button>
            ))}
          </div>
          <div className="destination-list">
            {dirs
              .filter((d) => !prompt.files.some((f) => f.id === d.id))
              .map((d) => {
                const probe = !enteredRepo && browsingCloudTree ? probes[d.id] : undefined;
                const isPlainRepo = probe?.state === 'repository' && probe.kind === 'plain';
                const isEncryptedRepo =
                  probe?.state === 'repository' && probe.kind === 'encrypted';
                return (
                  <button
                    key={d.id}
                    disabled={busy || isEncryptedRepo}
                    title={isEncryptedRepo ? '加密对象库不支持直接移入' : undefined}
                    onClick={() => void navigateDestination(d, [...destTrail, d])}
                  >
                    {isPlainRepo || isEncryptedRepo ? <AxuanStorMark /> : <Folder size={20} />}
                    <span className="dest-item-name">{d.name}</span>
                    {isPlainRepo && <span className="dest-repo-badge">对象库</span>}
                    {isEncryptedRepo && (
                      <span className="dest-repo-badge muted">加密库</span>
                    )}
                    <ChevronRight size={16} />
                  </button>
                );
              })}
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
            将 {prompt.files.length} 个项目{prompt.kind === 'copy' ? '复制' : '移动'}到
            {enteredRepo ? '对象库目录' : targetScope === 'cloud' ? '云盘目录' : ''}「{dest.name}」
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
