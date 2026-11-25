import {
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEventHandler,
  type ReactNode,
  type Ref,
} from 'react';
import {
  ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ChevronRight, Copy, File, FileArchive, FileImage,
  FileMusic, FileText, FileVideo, Folder, Loader2, MoreHorizontal, X,
} from 'lucide-react';
import { AxuanStorMark } from './RepositoryWorkspace';
import { OpenListCryptMark } from './CryptWorkspace';
import type { CloudFile } from './types';
import { dateText, fileKind, formatBytes } from './utils';

export type FileView = 'list' | 'grid' | 'grid-medium' | 'grid-small';
export const fileViewLabels: Record<FileView, string> = {
  list: '列表',
  grid: '大图标',
  'grid-medium': '中图标',
  'grid-small': '小图标',
};
export const fileGridSizes = {
  grid: { width: 152, mobileWidth: 128, height: 152, glyph: 42 },
  'grid-medium': { width: 120, mobileWidth: 104, height: 124, glyph: 32 },
  'grid-small': { width: 98, mobileWidth: 84, height: 104, glyph: 23 },
};
export type FileSort = { key: string; desc: boolean };

export interface AddressBarHandle {
  edit: () => void;
  isEditing: () => boolean;
  cancel: () => void;
}

export function FileBreadcrumbs({
  trail,
  rootIcon,
  onNavigate,
  onBack,
  backLabel = '关闭文件预览',
  pathValue,
  pathPlaceholder = '输入路径跳转…',
  onSubmitPath,
  onCopyPath,
  disabled = false,
  addressRef,
}: {
  trail: { id: string; name: string }[];
  rootIcon: ReactNode;
  onNavigate: (index: number) => void;
  onBack?: () => void;
  backLabel?: string;
  pathValue?: string;
  pathPlaceholder?: string;
  onSubmitPath?: (path: string) => Promise<boolean | void> | boolean | void;
  onCopyPath?: () => void;
  disabled?: boolean;
  addressRef?: Ref<AddressBarHandle>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(pathValue ?? '/');
  const [submitting, setSubmitting] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const crumbsRef = useRef<HTMLDivElement>(null);
  const editable = !!onSubmitPath && !disabled;

  const startEditing = () => {
    if (!onSubmitPath || disabled || submitting) return;
    setDraft(pathValue ?? '/');
    setEditing(true);
  };
  const cancelEditing = () => {
    if (submitting) return;
    setEditing(false);
  };

  useImperativeHandle(addressRef, () => ({
    edit: startEditing,
    isEditing: () => editing,
    cancel: cancelEditing,
  }));

  useEffect(() => {
    setEditing(false);
  }, [pathValue]);

  useLayoutEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    } else if (crumbsRef.current) {
      crumbsRef.current.scrollLeft = crumbsRef.current.scrollWidth;
    }
  }, [editing, trail]);

  const submit = async () => {
    if (!onSubmitPath || submitting) return;
    setSubmitting(true);
    try {
      const ok = await onSubmitPath(draft);
      if (ok !== false) setEditing(false);
      else {
        requestAnimationFrame(() => {
          inputRef.current?.focus();
          inputRef.current?.select();
        });
      }
    } finally {
      setSubmitting(false);
    }
  };

  if (!onSubmitPath && !onCopyPath) {
    return (
      <div className="breadcrumbs" ref={crumbsRef}>
        {(trail.length > 1 || onBack) && (
          <button
            type="button"
            className="icon-button"
            data-preview-back={onBack ? true : undefined}
            aria-label={trail.length > 1 ? '返回上级目录' : backLabel}
            onClick={trail.length > 1 ? () => onNavigate(trail.length - 2) : onBack}
          >
            <ArrowLeft size={16} />
          </button>
        )}
        {trail.map((dir, index) => (
          <span key={dir.id}>
            <button type="button" disabled={index === trail.length - 1} onClick={() => onNavigate(index)}>
              {index === 0 && rootIcon}
              {dir.name}
            </button>
            {index < trail.length - 1 && <ChevronRight size={14} />}
          </span>
        ))}
      </div>
    );
  }

  return (
    <div className="address-bar-shell">
      {(trail.length > 1 || onBack) && !editing && (
        <button
          type="button"
          className="icon-button breadcrumb-back"
          data-preview-back={onBack ? true : undefined}
          aria-label={trail.length > 1 ? '返回上级目录' : backLabel}
          disabled={disabled}
          onClick={trail.length > 1 ? () => onNavigate(trail.length - 2) : onBack}
        >
          <ArrowLeft size={16} />
        </button>
      )}
      {editing ? (
        <form
          className="address-bar is-editing"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
          onBlur={(event) => {
            if (!submitting && !event.currentTarget.contains(event.relatedTarget as Node | null)) {
              setEditing(false);
            }
          }}
        >
          <span className="address-leading-icon" aria-hidden="true">
            {rootIcon}
          </span>
          <input
            ref={inputRef}
            className="address-input"
            type="text"
            value={draft}
            aria-label="路径地址栏"
            placeholder={pathPlaceholder}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            disabled={submitting}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                cancelEditing();
              }
            }}
          />
          <button
            type="submit"
            className="icon-button address-action address-submit"
            aria-label="前往路径"
            title="前往路径 (Enter)"
            disabled={submitting}
            onMouseDown={(event) => event.preventDefault()}
          >
            {submitting ? <Loader2 size={15} className="spin" /> : <ArrowRight size={15} />}
          </button>
          <button
            type="button"
            className="icon-button address-action"
            aria-label="取消编辑路径"
            title="取消 (Esc)"
            disabled={submitting}
            onMouseDown={(event) => event.preventDefault()}
            onClick={cancelEditing}
          >
            <X size={15} />
          </button>
        </form>
      ) : (
        <div
          className={'address-bar' + (editable ? ' editable' : '')}
          onClick={(event) => {
            if (!editable) return;
            if ((event.target as HTMLElement).closest('button')) return;
            startEditing();
          }}
        >
          <div className="breadcrumbs" ref={crumbsRef}>
            {trail.map((dir, index) => (
              <span key={dir.id}>
                <button
                  type="button"
                  disabled={index === trail.length - 1}
                  onClick={() => onNavigate(index)}
                >
                  {index === 0 && rootIcon}
                  {dir.name}
                </button>
                {index < trail.length - 1 && <ChevronRight size={14} />}
              </span>
            ))}
            {editable && (
              <span
                className="address-bar-blank"
                aria-hidden="true"
                title="点击输入路径 (Ctrl+L)"
              />
            )}
          </div>
          {onCopyPath && (
            <button
              type="button"
              className="icon-button address-action address-copy"
              aria-label="复制地址栏路径"
              title="复制当前路径"
              onClick={(event) => {
                event.stopPropagation();
                onCopyPath();
              }}
            >
              <Copy size={14} />
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export function FileTableHead({
  sort, onSort, allSelected, partiallySelected, disableSelectAll, onSelectAll,
}: {
  sort: FileSort;
  onSort: (sort: FileSort) => void;
  allSelected: boolean;
  partiallySelected?: boolean;
  disableSelectAll?: boolean;
  onSelectAll: (checked: boolean) => void;
}) {
  return (
    <div className="table-head">
      <FileSelectAll checked={allSelected} partiallySelected={partiallySelected}
        disabled={disableSelectAll} onChange={onSelectAll} />
      {[
        ['name', '文件名称'],
        ['size', '大小'],
        ['modifiedAt', '修改时间'],
      ].map(([key, label]) => (
        <button key={key} className={'col-' + key}
          onClick={() => onSort({ key, desc: sort.key === key ? !sort.desc : false })}>
          {label}
          {sort.key === key && (sort.desc ? <ArrowDown size={13} /> : <ArrowUp size={13} />)}
        </button>
      ))}
      <span className="col-menu" />
    </div>
  );
}

export function FileSelectAll({
  checked, partiallySelected, disabled, onChange,
}: {
  checked: boolean;
  partiallySelected?: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const checkbox = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (checkbox.current) checkbox.current.indeterminate = !!partiallySelected && !checked;
  }, [partiallySelected, checked]);
  return (
    <label className="checkbox-wrap">
      <input ref={checkbox} type="checkbox" aria-label="选择所有已加载文件"
        checked={checked} disabled={disabled}
        onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function FileGlyph({
  file,
  size = 22,
  repository = false,
  encryptedFolder = false,
}: {
  file: CloudFile;
  size?: number;
  repository?: boolean;
  encryptedFolder?: boolean;
}) {
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
      {repository ? (
        <span className="repository-badge">
          <AxuanStorMark />
        </span>
      ) : (
        encryptedFolder && (
          <span className="repository-badge crypt-badge">
            <OpenListCryptMark />
          </span>
        )
      )}
    </span>
  );
}


export function FileRowContents({
  file, view, smallScreen, repository, encryptedFolder, selected, onToggle, selectionDisabled, sizeUnavailable, location, onOpen, action,
}: {
  file: CloudFile;
  view: FileView;
  smallScreen: boolean;
  repository?: boolean;
  encryptedFolder?: boolean;
  selected: boolean;
  onToggle: () => void;
  selectionDisabled?: boolean;
  sizeUnavailable?: boolean;
  location?: string;
  onOpen?: MouseEventHandler<HTMLButtonElement>;
  action: ReactNode;
}) {
  const grid = view === 'list' ? null : fileGridSizes[view];
  const glyph = <FileGlyph file={file} repository={repository} encryptedFolder={encryptedFolder} size={grid?.glyph ?? (smallScreen ? 28 : 22)} />;
  return (
    <>
      <label className="checkbox-wrap">
        <input type="checkbox" aria-label={'选择 ' + file.name} checked={selected}
          disabled={selectionDisabled} onChange={onToggle} />
      </label>
      <button className="file-name" title={location ? location + '/' + file.name : file.name}
        aria-label={file.name} onClick={onOpen}>
        {grid && file.thumbnail ? (
          <span className="file-artwork">
            <img className="file-thumbnail" src={file.thumbnail} alt="" loading="lazy" draggable={false}
              onError={(event) => { event.currentTarget.hidden = true; }} />
            {glyph}
          </span>
        ) : glyph}
        <span>
          <strong>{file.name}</strong>
          {view === 'list' && (
            <small>
              {location ?? (smallScreen
                ? (file.isDir ? '文件夹' : sizeUnavailable ? fileKind(file) : formatBytes(file.size)) + (file.modifiedAt ? ' · ' + dateText(file.modifiedAt) : '')
                : fileKind(file))}
            </small>
          )}
        </span>
      </button>
      {!(grid && file.isDir) && (
        <span className="col-size">{file.isDir || sizeUnavailable ? '—' : formatBytes(file.size)}</span>
      )}
      <span className="col-modifiedAt">{dateText(file.modifiedAt)}</span>
      {action}
    </>
  );
}

export function FileRowMenuButton({
  file, disabled, mobile, onOpen,
}: {
  file: CloudFile;
  disabled?: boolean;
  mobile?: boolean;
  onOpen: (file: CloudFile, x: number, y: number) => void;
}) {
  return (
    <button className="icon-button row-more" aria-label={file.name + ' 操作菜单'}
      aria-haspopup={mobile ? 'dialog' : 'menu'} disabled={disabled}
      onClick={(event) => {
        const bounds = event.currentTarget.getBoundingClientRect();
        onOpen(file, bounds.right, bounds.bottom);
      }}>
      <MoreHorizontal size={19} />
    </button>
  );
}
