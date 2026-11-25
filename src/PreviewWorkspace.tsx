import { useEffect, useId, useRef, type ReactNode } from 'react';
import { ArrowLeft, Download } from 'lucide-react';
import './contentPreview.css';

export function PreviewWorkspace({
  name, detail, onClose, onDownload, hideHeader, children,
}: {
  name: string;
  detail?: string;
  onClose: () => void;
  onDownload: () => void;
  hideHeader?: boolean;
  children: ReactNode;
}) {
  const title = useId();
  const workspace = useRef<HTMLElement>(null);
  useEffect(() => {
    workspace.current?.querySelector<HTMLButtonElement>('[data-preview-back]')?.focus({ preventScroll: true });
  }, []);
  return (
    <section ref={workspace} className="preview-workspace"
      aria-labelledby={hideHeader ? undefined : title} aria-label={hideHeader ? name : undefined}>
      {!hideHeader && <header className="preview-topbar">
        <button type="button" data-preview-back className="icon-button preview-back"
          aria-label="关闭文件预览" title="返回文件列表" onClick={onClose}><ArrowLeft size={20} /></button>
        <div className="preview-title">
          <h1 id={title} title={name}>{name}</h1>
          {detail && <p>{detail}</p>}
        </div>
        <button type="button" className="button secondary preview-download"
          aria-label="下载文件" title="下载文件" onClick={onDownload}>
          <Download size={16} /><span>下载文件</span>
        </button>
      </header>}
      {children}
    </section>
  );
}
