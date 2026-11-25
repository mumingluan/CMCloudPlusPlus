import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { ChevronLeft, ChevronRight, Download, FileArchive, Folder, Info, Loader2, Search, SlidersHorizontal, X } from 'lucide-react';
import Markdown from 'react-markdown';
import { errorText } from './api';
import {
  contentPreviewApi, officePreviewLimit, pdfPreviewLimit, textPreviewLimit,
  type ArchiveEntry, type ArchiveView, type ContentKind, type PreviewSource,
} from './contentPreviewApi';
import { PdfPreview } from './PdfPreview';
import { OfficePreview } from './OfficePreview';
import { PreviewWorkspace } from './PreviewWorkspace';
import type { RepositoryView } from './repositoryApi';
import type { CryptView } from './cryptApi';
import type { CloudFile } from './types';
import { fileKind, formatBytes, sortFiles } from './utils';
import { FileBreadcrumbs, FileRowContents, FileRowMenuButton, FileSelectAll, FileTableHead,
  fileGridSizes, type FileSort, type FileView } from './FileBrowserParts';
import { FileContextMenu } from './FileContextMenu';
import { FileViewOptions } from './FileViewOptions';
import { useMobileFileLongPress } from './useMobileFileLongPress';

const pageSize = 150;

function indexArchive(entries: ArchiveEntry[]) {
  const result = new Map<string, ArchiveEntry>();
  for (const entry of entries) {
    if (!entry.directory) result.set('file:' + entry.id, entry);
    let path = entry.directory ? entry.path : entry.path.slice(0, Math.max(0, entry.path.lastIndexOf('/')));
    while (path) {
      const key = 'folder:' + path;
      const folder = result.get(key) ?? { id: -1, path, size: null, directory: true, downloadable: true };
      if (!entry.directory && !entry.downloadable) folder.downloadable = false;
      result.set(key, folder);
      path = path.slice(0, Math.max(0, path.lastIndexOf('/')));
    }
  }
  return result;
}

function ArchiveBrowser({
  archive, name, smallScreen, initialView, state, onClose, onDownload,
}: {
  archive: ArchiveView | null;
  name: string;
  smallScreen: boolean;
  initialView: FileView;
  state?: ReactNode;
  onClose: () => void;
  onDownload: () => void;
}) {
  const [directory, setDirectory] = useState('');
  const [view, setView] = useState(initialView);
  const grid = view === 'list' ? null : fileGridSizes[view];
  const [viewOptions, setViewOptions] = useState(false);
  const [sort, setSort] = useState<FileSort>({ key: 'name', desc: false });
  const [filter, setFilter] = useState('all');
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [mobileSelecting, setMobileSelecting] = useState(false);
  const selecting = selection.size > 0 || (smallScreen && mobileSelecting);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState('');
  const [error, setError] = useState('');
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  const closeOptions = useCallback(() => setViewOptions(false), []);
  const scroll = useRef<HTMLDivElement>(null);
  const anchor = useRef<string | null>(null);
  const entries = useMemo(() => indexArchive(archive?.entries ?? []), [archive]);
  const prefix = directory ? directory + '/' : '';
  const keyword = query.trim().toLocaleLowerCase();
  const folderCount = useMemo(() => {
    let count = 0;
    for (const entry of entries.values()) {
      if (entry.path.startsWith(prefix) && entry.path.length > prefix.length &&
        !entry.path.includes('/', prefix.length)) count++;
    }
    return count;
  }, [entries, prefix]);
  const rows = useMemo(() => sortFiles(
    Array.from(entries).flatMap(([id, entry]): CloudFile[] => {
      if (!entry.path.startsWith(prefix) || entry.path.length <= prefix.length ||
        (!keyword && entry.path.includes('/', prefix.length))) return [];
      const file = {
        id,
        path: entry.path,
        name: entry.path.slice(entry.path.lastIndexOf('/') + 1),
        isDir: entry.directory,
        size: entry.size ?? 0,
        modifiedAt: '',
      };
      if (!file.name.toLocaleLowerCase().includes(keyword) ||
        (filter !== 'all' && (filter === 'folders' ? !file.isDir : fileKind(file) !== filter))) return [];
      return [file];
    }),
    sort.key, sort.desc,
  ), [entries, prefix, keyword, filter, sort]);
  const visible = rows.slice(page * pageSize, (page + 1) * pageSize);
  const selectable = visible.filter((file) => entries.get(file.id)?.downloadable);
  const allSelected = selectable.length > 0 && selectable.every((file) => selection.has(file.id));
  const crumbs = [{ id: '', name }, ...(directory ? directory.split('/').map((part, index, parts) => ({
    id: parts.slice(0, index + 1).join('/'), name: part,
  })) : [])];
  const menuEntry = menu ? entries.get(menu.id) : undefined;
  const longPress = useMobileFileLongPress(smallScreen, (id) => {
    setMobileSelecting(true);
    if (entries.get(id)?.downloadable) setSelection((old) => new Set([...old, id]));
  }, directory);

  function clearSelection() {
    setMobileSelecting(false);
    setSelection(new Set());
    anchor.current = null;
  }

  function resetPage() {
    setPage(0);
    clearSelection();
    if (scroll.current) scroll.current.scrollTop = 0;
  }

  function navigate(path: string) {
    setDirectory(path);
    setQuery('');
    setFilter('all');
    resetPage();
  }

  useEffect(() => {
    const back = (event: Event) => {
      if (menu) closeMenu();
      else if (viewOptions) closeOptions();
      else if (selecting) clearSelection();
      else if (directory) navigate(directory.slice(0, Math.max(0, directory.lastIndexOf('/'))));
      else return;
      event.preventDefault();
    };
    window.addEventListener('cmcloud-preview-back', back);
    return () => window.removeEventListener('cmcloud-preview-back', back);
  }, [directory, selecting, menu, viewOptions, closeMenu, closeOptions]);

  function toggle(ids: string[], enabled: boolean) {
    const next = new Set(selection);
    for (const id of ids) {
      if (enabled) next.add(id);
      else next.delete(id);
    }
    setSelection(next);
    if (next.size === 0) setMobileSelecting(false);
  }

  async function save(items: ArchiveEntry[]) {
    if (!archive || saving || !items.length) return;
    setSaving(true);
    setNotice('');
    setError('');
    try {
      const singleFile = items.length === 1 && !items[0].directory;
      const saved = singleFile
        ? await contentPreviewApi.save(archive.id, items[0].id)
        : await contentPreviewApi.saveSelection(archive.id, items.map((item) => item.path));
      if (saved) setNotice(singleFile ? '文件已保存。' : '所选内容已保存为 ZIP。');
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setSaving(false);
    }
  }

  function openMenu(file: CloudFile, x: number, y: number) {
    setMenu({ id: file.id, x, y });
  }

  function openRow(file: CloudFile) {
    if (file.isDir) navigate(file.path);
    else selectRow(file, false);
  }

  function selectRow(file: CloudFile, shift: boolean) {
    if (!entries.get(file.id)?.downloadable) return;
    const from = shift ? rows.findIndex((row) => row.id === anchor.current) : -1;
    if (from < 0) toggle([file.id], !selection.has(file.id));
    else {
      const to = rows.indexOf(file);
      toggle(rows.slice(Math.min(from, to), Math.max(from, to) + 1)
        .filter((row) => entries.get(row.id)?.downloadable).map((row) => row.id), true);
    }
    anchor.current = file.id;
  }

  return (
    <div className={'workspace files-workspace archive-browser' + (selecting ? ' selecting-files' : '')}>
      <div className="file-toolbar">
        {selecting ? (
          <div className="toolbar-actions mobile-file-actions">
            {smallScreen && <FileSelectAll checked={allSelected}
              partiallySelected={selectable.some((file) => selection.has(file.id))}
              disabled={!selectable.length} onChange={(checked) => toggle(selectable.map((file) => file.id), checked)} />}
            <span className="selected-count">已选 {selection.size} 项</span>
            <button type="button" className="button quiet" disabled={saving || !selection.size}
              aria-label="下载选中项" onClick={() => void save([...selection].map((id) => entries.get(id)!))}>
              <Download size={16} /><span>下载</span>
            </button>
            <button type="button" className="icon-button" aria-label="取消选择" onClick={clearSelection}>
              <X size={16} />
            </button>
          </div>
        ) : (
          <>
            <div className="file-search">
              <label className="search">
                <Search size={17} />
                <input type="search" aria-label="搜索" placeholder="搜索" value={query} disabled={!archive}
                  onChange={(event) => { setQuery(event.target.value); resetPage(); }} />
              </label>
            </div>
            <div className="toolbar-actions mobile-file-actions">
              <button type="button" className="icon-button file-toolbar-download" aria-label="下载压缩包"
                onClick={onDownload}><Download size={19} /><span>下载</span></button>
              <button type="button" className="icon-button" aria-label="文件显示选项"
                aria-haspopup="dialog" aria-expanded={viewOptions} disabled={!archive} onClick={() => setViewOptions(true)}>
                <SlidersHorizontal size={20} />
              </button>
            </div>
          </>
        )}
      </div>
      <div className="breadcrumb-bar">
        <FileBreadcrumbs
          trail={crumbs}
          rootIcon={<FileArchive size={16} />}
          onNavigate={(index) => navigate(crumbs[index].id)}
          onBack={onClose}
          pathValue={'/' + directory}
          pathPlaceholder="输入压缩包内路径…"
          disabled={!archive}
          onCopyPath={() =>
            void navigator.clipboard
              .writeText('/' + directory)
              .then(() => setNotice('当前路径已复制。'))
              .catch((reason) => setError(errorText(reason)))
          }
          onSubmitPath={(raw) => {
            const parts: string[] = [];
            for (const item of raw.trim().replace(/\\+/g, '/').split('/')) {
              const part = item.trim();
              if (!part || part === '.') continue;
              if (part === '..') parts.pop();
              else parts.push(part);
            }
            const target = parts.join('/');
            if (!target) {
              navigate('');
              return true;
            }
            const folderPrefix = target + '/';
            let isDir = false;
            let fileId: string | null = null;
            for (const [id, entry] of entries) {
              if (entry.path === target) {
                if (entry.directory) isDir = true;
                else fileId = id;
              } else if (entry.path.startsWith(folderPrefix)) {
                isDir = true;
              }
            }
            if (isDir) {
              navigate(target);
              return true;
            }
            if (fileId !== null) {
              const slash = target.lastIndexOf('/');
              navigate(slash < 0 ? '' : target.slice(0, slash));
              if (entries.get(fileId)?.downloadable) setSelection(new Set([fileId]));
              return true;
            }
            setError(`压缩包内不存在路径：/${target}`);
            return false;
          }}
        />
        {!smallScreen && <div className="view-options"><span className="space-tag">只读</span></div>}
      </div>
      {state ?? <div className="files-area">
        <div className={'file-table ' + (grid ? 'icon-view ' + view : 'list-view')}>
          {!smallScreen && <FileTableHead sort={sort} onSort={(next) => { setSort(next); resetPage(); }}
            allSelected={allSelected} partiallySelected={selectable.some((file) => selection.has(file.id))}
            disableSelectAll={!selectable.length} onSelectAll={(checked) => toggle(selectable.map((file) => file.id), checked)} />}
          <div className="file-scroll" ref={scroll} role="region" aria-label="目录内容" tabIndex={0}
            onKeyDown={(event) => {
              if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
                event.preventDefault();
                toggle(selectable.map((file) => file.id), true);
              }
            }}>
            {rows.length === 0 ? (
              <div className="empty-folder">
                <Folder size={47} strokeWidth={1.2} />
                <h2>{query.trim() || filter !== 'all' ? '没有匹配的文件' : '这个文件夹还是空的'}</h2>
              </div>
            ) : (
              <div className="archive-rows" style={grid ? {
                gridTemplateColumns: `repeat(auto-fill, minmax(min(100%, ${smallScreen ? grid.mobileWidth : grid.width}px), 1fr))`,
                gridAutoRows: grid.height,
              } : undefined}>
                {visible.map((file) => {
                  const location = file.path.slice(prefix.length, Math.max(prefix.length, file.path.lastIndexOf('/')));
                  return (
                  <div key={file.id}
                    className={'file-row' + (selection.has(file.id) ? ' selected' : '') +
                      (grid ? ' file-card' : location ? ' archive-search-result' : smallScreen ? '' : ' compact')}
                    onPointerDown={(event) => longPress.start(file.id, event)}
                    onPointerMove={longPress.move} onPointerUp={longPress.cancel} onPointerCancel={longPress.cancel}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      if (smallScreen) longPress.context(file.id);
                      else openMenu(file, event.clientX, event.clientY);
                    }}
                    onClick={(event) => {
                      if ((event.target as HTMLElement).closest('button, label, input')) return;
                      if (longPress.consumeClick(file.id)) return;
                      if (selecting || event.shiftKey || event.ctrlKey || event.metaKey) selectRow(file, event.shiftKey);
                      else openRow(file);
                    }}>
                    <FileRowContents file={file} view={view} smallScreen={smallScreen}
                      selected={selection.has(file.id)} selectionDisabled={!entries.get(file.id)?.downloadable}
                      sizeUnavailable={entries.get(file.id)?.size == null}
                      location={location || undefined}
                      onToggle={() => selectRow(file, false)}
                      onOpen={(event) => {
                        if (longPress.consumeClick(file.id)) return;
                        if ((smallScreen && selecting) || event.shiftKey || event.ctrlKey || event.metaKey)
                          selectRow(file, event.shiftKey);
                        else openRow(file);
                      }}
                      action={<FileRowMenuButton file={file} mobile={smallScreen} onOpen={openMenu} />} />
                  </div>
                  );
                })}
              </div>
            )}
          </div>
          <footer className="file-footer archive-footer">
            <span>已加载 {folderCount.toLocaleString()} 项{keyword || filter !== 'all' ? ' · ' + rows.length + ' 项匹配' : ''}</span>
            {saving && <span role="status"><Loader2 size={15} className="spin" />正在提取并保存…</span>}
            {notice && <span role="status">{notice}</span>}
            {error && <span className="archive-error" role="alert">{error}</span>}
            {rows.length > pageSize && <div className="archive-pages">
              <span>{page * pageSize + 1}–{Math.min((page + 1) * pageSize, rows.length)}</span>
              <button type="button" className="icon-button" disabled={page === 0} aria-label="上一页"
                onClick={() => { setPage(page - 1); if (scroll.current) scroll.current.scrollTop = 0; }}><ChevronLeft size={17} /></button>
              <button type="button" className="icon-button" disabled={(page + 1) * pageSize >= rows.length} aria-label="下一页"
                onClick={() => { setPage(page + 1); if (scroll.current) scroll.current.scrollTop = 0; }}><ChevronRight size={17} /></button>
            </div>}
          </footer>
        </div>
      </div>}
      {viewOptions && <FileViewOptions view={view} setView={setView} sort={sort}
        setSort={(next) => { setSort(next); resetPage(); }} filter={filter}
        setFilter={(next) => { setFilter(next); resetPage(); }} onClose={closeOptions} />}
      {menu && menuEntry && (
        <FileContextMenu x={menu.x} y={menu.y} mobile={smallScreen}
          target={menuEntry.path.slice(menuEntry.path.lastIndexOf('/') + 1)}
          onClose={closeMenu} items={[
            ...(menuEntry.directory ? [{ label: '打开文件夹', icon: Folder, action: () => navigate(menuEntry.path) }] : []),
            { label: '下载', icon: Download, disabled: saving || !menuEntry.downloadable, action: () => void save([menuEntry]) },
          ]} />
      )}
    </div>
  );
}

export function FileContentPreview({
  file, kind, repository, crypt, onClose, onDownload, smallScreen = false, initialFileView = 'list',
}: {
  file: CloudFile;
  kind: ContentKind;
  repository?: RepositoryView;
  crypt?: CryptView;
  onClose: () => void;
  onDownload: () => void;
  smallScreen?: boolean;
  initialFileView?: FileView;
}) {
  const handle = repository?.handle;
  const revision = repository?.revision;
  const cryptHandle = crypt?.handle;
  const source = useMemo<PreviewSource>(
    () =>
      handle && revision
        ? { kind: 'repository', handle, revision, path: file.path }
        : cryptHandle
          ? { kind: 'crypt', handle: cryptHandle, path: file.path }
          : { kind: 'cloud', file },
    [handle, revision, cryptHandle, file],
  );
  const pending = useRef<Promise<ArchiveView | string> | null>(null);
  const cleanup = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [archive, setArchive] = useState<ArchiveView | null>(null);
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [rawMarkdown, setRawMarkdown] = useState(false);
  const limit = kind === 'pdf' ? pdfPreviewLimit : kind === 'office' ? officePreviewLimit : textPreviewLimit;
  const oversized = kind !== 'archive' && file.size > limit;
  const type = kind === 'markdown' ? 'Markdown' : kind === 'text' ? '纯文本'
      : kind === 'pdf' ? 'PDF' : file.name.split('.').pop()?.toUpperCase() ?? 'Office';

  useEffect(() => {
    if (oversized || kind === 'pdf' || kind === 'office') return;
    let closed = false;
    if (cleanup.current !== null) clearTimeout(cleanup.current);
    const work = pending.current ??= kind === 'archive' ? contentPreviewApi.open(source) : contentPreviewApi.text(source);
    void work.then((value) => {
      if (closed) return;
      if (typeof value === 'string') setText(value);
      else setArchive(value);
    }, (reason) => {
      if (!closed) setError(errorText(reason));
    }).finally(() => {
      if (!closed) setLoaded(true);
    });
    return () => {
      closed = true;
      if (kind === 'archive') {
        cleanup.current = setTimeout(() => {
          void work.then((value) => typeof value === 'string' ? undefined : contentPreviewApi.close(value.id)).catch(() => {});
        }, 0);
      }
    };
  }, [kind, oversized, source]);

  if (kind === 'archive') {
    return <PreviewWorkspace name={file.name} onClose={onClose} onDownload={onDownload} hideHeader>
      <ArchiveBrowser archive={archive} name={file.name} smallScreen={smallScreen} initialView={initialFileView}
        onClose={onClose} onDownload={onDownload}
        state={error ? <div className="preview-state" role="alert"><Info size={20} />{error}</div>
          : !loaded ? <p className="preview-state" role="status"><Loader2 size={18} className="spin" />正在读取压缩包目录…</p>
            : undefined} />
    </PreviewWorkspace>;
  }

  return <PreviewWorkspace name={file.name} detail={type + ' · ' + formatBytes(file.size)}
    onClose={onClose} onDownload={onDownload}>
    {oversized || error
      ? <div className="preview-state" role="alert"><Info size={20} />
        {oversized ? type + '超过 ' + formatBytes(limit) + ' 预览上限，请下载后打开。' : error}</div>
      : kind === 'pdf' ? <PdfPreview source={source} />
        : kind === 'office' ? <OfficePreview source={source} />
          : !loaded ? <p className="preview-state" role="status"><Loader2 size={18} className="spin" />
            正在读取文本…</p>
            : <div className="preview-reading">
                {kind === 'markdown' && <div className="markdown-mode" aria-label="Markdown 查看方式">
                  <button type="button" className="quiet-button" aria-pressed={!rawMarkdown} onClick={() => setRawMarkdown(false)}>渲染</button>
                  <button type="button" className="quiet-button" aria-pressed={rawMarkdown} onClick={() => setRawMarkdown(true)}>源文本</button>
                </div>}
                {kind === 'markdown' && !rawMarkdown
                  ? <article className="markdown-preview" tabIndex={0}><Markdown skipHtml components={{
                    a: ({ children }) => <span>{children}</span>,
                    img: ({ alt }) => alt ? <span>{alt}</span> : null,
                  }}>{text}</Markdown></article>
                  : <pre className="text-preview" tabIndex={0}>{text || '空文件'}</pre>}
              </div>}
  </PreviewWorkspace>;
}
