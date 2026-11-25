import { useEffect, useMemo, useRef, useState } from 'react';
import { ChevronLeft, ChevronRight, Info, Loader2, Minus, Plus } from 'lucide-react';
import type { PDFDocumentLoadingTask, PDFDocumentProxy, RenderTask } from 'pdfjs-dist';
import { contentPreviewApi, type PreviewSource } from './contentPreviewApi';
import { errorText } from './api';

const assetRoot = import.meta.env.DEV ? 'node_modules/pdfjs-dist/' : 'pdf-assets/';

export function PdfPreview({ source }: { source: PreviewSource }) {
  const [document, setDocument] = useState<PDFDocumentProxy>();
  const [error, setError] = useState('');
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState(100);
  const [width, setWidth] = useState(600);
  const [rendering, setRendering] = useState(false);
  const area = useRef<HTMLDivElement>(null);
  const output = useRef<HTMLDivElement>(null);
  const request = useMemo(() => ({ pending: undefined as Promise<ArrayBuffer> | undefined }), [source]);

  useEffect(() => {
    let closed = false;
    let task: PDFDocumentLoadingTask | undefined;
    request.pending ??= contentPreviewApi.pdf(source);
    void Promise.all([request.pending, import('pdfjs-dist/legacy/build/pdf.mjs'),
      import('pdfjs-dist/legacy/build/pdf.worker.mjs?url')]).then(async ([bytes, pdf, worker]) => {
      if (closed) return;
      pdf.GlobalWorkerOptions.workerSrc = worker.default;
      const assets = new URL(assetRoot, window.location.href);
      task = pdf.getDocument({
        data: new Uint8Array(bytes),
        cMapUrl: new URL('cmaps/', assets).href,
        cMapPacked: true,
        standardFontDataUrl: new URL('standard_fonts/', assets).href,
        wasmUrl: new URL('wasm/', assets).href,
        enableXfa: false,
        maxImageSize: 24000000,
      });
      task.onPassword = () => {
        if (!closed) setError('此 PDF 受密码保护，请下载后打开。');
        void task?.destroy().catch(() => {});
      };
      const result = await task.promise;
      if (!closed) setDocument(result);
    }).catch((reason) => {
      if (!closed) setError((current) => current || errorText(reason));
    });
    return () => {
      closed = true;
      void task?.destroy().catch(() => {});
    };
  }, [request, source]);

  useEffect(() => {
    const container = area.current;
    if (!document || !container) return;
    const measure = () => setWidth(Math.max(240, container.clientWidth - 32));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    return () => observer.disconnect();
  }, [document]);

  useEffect(() => {
    if (!document || !output.current) return;
    let closed = false;
    let render: RenderTask | undefined;
    const host = output.current;
    setRendering(true);
    setError('');
    void document.getPage(page).then(async (pdfPage) => {
      if (closed) return;
      const original = pdfPage.getViewport({ scale: 1 });
      const ratio = Math.min(width, 980) / original.width * zoom / 100;
      const view = pdfPage.getViewport({ scale: ratio });
      const pixels = Math.min(window.devicePixelRatio || 1, 2, 2500 / Math.max(view.width, view.height));
      const canvas = window.document.createElement('canvas');
      canvas.width = Math.max(1, Math.floor(view.width * pixels));
      canvas.height = Math.max(1, Math.floor(view.height * pixels));
      canvas.style.width = view.width + 'px';
      canvas.style.height = view.height + 'px';
      canvas.setAttribute('aria-label', 'PDF 第 ' + page + ' 页');
      render = pdfPage.render({
        canvas, viewport: view, transform: [pixels, 0, 0, pixels, 0, 0],
        annotationMode: 0,
      });
      await render.promise;
      if (!closed) host.replaceChildren(canvas);
    }).catch(() => {
      if (!closed) setError('此页无法渲染，请下载后打开。');
    }).finally(() => {
      if (!closed) setRendering(false);
    });
    return () => { closed = true; render?.cancel(); };
  }, [document, page, width, zoom]);

  if (!document) return error
    ? <p className="preview-state" role="alert"><Info size={19} />{error}</p>
    : <p className="preview-state" role="status"><Loader2 size={18} className="spin" />正在读取 PDF…</p>;
  return <>
    <div className="document-controls">
      <div className="document-pager">
        <button type="button" className="icon-button" aria-label="上一页" disabled={page <= 1}
          onClick={() => setPage(page - 1)}><ChevronLeft size={19} /></button>
        <span aria-live="polite">{page} / {document.numPages}</span>
        <button type="button" className="icon-button" aria-label="下一页" disabled={page >= document.numPages}
          onClick={() => setPage(page + 1)}><ChevronRight size={19} /></button>
      </div>
      <div className="document-pager">
        <button type="button" className="icon-button" aria-label="缩小" disabled={zoom <= 50}
          onClick={() => setZoom(zoom - 25)}><Minus size={18} /></button>
        <button type="button" className="quiet-button" title="适合窗口" onClick={() => setZoom(100)}>{zoom}%</button>
        <button type="button" className="icon-button" aria-label="放大" disabled={zoom >= 200}
          onClick={() => setZoom(zoom + 25)}><Plus size={18} /></button>
      </div>
      {rendering && <Loader2 size={16} className="spin" aria-label="正在渲染" />}
    </div>
    {error && <p className="preview-state" role="alert"><Info size={19} />{error}</p>}
    <div className="pdf-scroll" ref={area} role="region" aria-label="PDF 页面" tabIndex={0}>
      <div className="pdf-page" ref={output} />
    </div>
  </>;
}
