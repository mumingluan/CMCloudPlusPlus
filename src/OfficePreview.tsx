import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { ChevronLeft, ChevronRight, Info, Loader2 } from 'lucide-react';
import { contentPreviewApi, type OfficeParts, type PreviewSource } from './contentPreviewApi';
import { errorText } from './api';
import { PreviewZoom } from './PreviewZoom';

const children = (element?: Element | null, name?: string) =>
  Array.from(element?.children ?? []).filter((child) => !name || child.localName === name);
const descendants = (element: Element | undefined, name: string) =>
  Array.from(element?.getElementsByTagNameNS('*', name) ?? []);
const first = (element: Element | undefined, name: string) => descendants(element, name)[0];
const localAttribute = (element: Element, name: string) =>
  Array.from(element.attributes).find((attr) => attr.localName === name)?.value;
const relationshipId = (element: Element) => Array.from(element.attributes).find((attr) =>
  attr.localName === 'id' && attr.namespaceURI?.endsWith('/relationships'))?.value ?? '';
const textOf = (element: Element | undefined) => descendants(element, 't').map((node) => node.textContent ?? '').join('');

function xml(parts: OfficeParts, key: string) {
  const value = parts.xml[key];
  if (typeof value !== 'string' || /<!DOCTYPE|<!ENTITY/i.test(value)) return undefined;
  const doc = new DOMParser().parseFromString(value, 'application/xml');
  return doc.getElementsByTagName('parsererror').length ? undefined : doc.documentElement;
}

function relations(parts: OfficeParts, part: string) {
  const slash = part.lastIndexOf('/');
  const parent = part.slice(0, slash);
  const result = new Map<string, { path: string; type: string }>();
  for (const item of children(xml(parts, parent + '/_rels/' + part.slice(slash + 1) + '.rels'), 'Relationship')) {
    const target = item.getAttribute('Target') ?? '';
    if (!target || item.getAttribute('TargetMode')?.toLowerCase() === 'external' || /[:\\?#]/.test(target)) continue;
    const path = target.startsWith('/') ? [] : parent.split('/');
    for (const component of target.split('/')) {
      if (component === '..') path.pop();
      else if (component && component !== '.') path.push(component);
    }
    if (path[0] === parent.split('/')[0]) {
      const id = item.getAttribute('Id');
      if (id) result.set(id, { path: path.join('/'), type: item.getAttribute('Type') ?? '' });
    }
  }
  return result;
}

function inlineImage(parts: OfficeParts, related: ReturnType<typeof relations>, element: Element) {
  const blip = first(element, 'blip');
  const target = blip && related.get(localAttribute(blip, 'embed') ?? '');
  return target && parts.images[target.path];
}

function WordPage({ parts }: { parts: OfficeParts }) {
  const main = 'word/document.xml';
  const body = first(xml(parts, main), 'body');
  if (!body) throw new Error('Word 正文不可用，请下载后打开。');
  const related = relations(parts, main);
  function renderBlock(block: Element, index: number, depth = 0): ReactNode {
    if (depth > 12) return null;
    if (block.localName === 'tbl') {
      return <table key={index} className="office-word-table"><tbody>
        {children(block, 'tr').map((row, r) => <tr key={r}>
          {children(row, 'tc').map((cell, c) => <td key={c}>{children(cell).map((part, i) => renderBlock(part, i, depth + 1))}</td>)}
        </tr>)}
      </tbody></table>;
    }
    if (block.localName !== 'p') return null;
    const style = first(block, 'pStyle');
    const heading = style && /^(?:heading|标题)\s*([1-6])$/i.exec(localAttribute(style, 'val') ?? '')?.[1];
    const runs = descendants(block, 'r').map((run, id) => {
      const emphasis = first(run, 'rPr');
      const bold = first(emphasis, 'b');
      const italic = first(emphasis, 'i');
      const on = (node: Element | undefined) => !!node && !['0', 'false', 'off'].includes(localAttribute(node, 'val') ?? '');
      return <span key={id} style={{ fontWeight: on(bold) ? 650 : undefined, fontStyle: on(italic) ? 'italic' : undefined }}>
        {children(run).map((node, n) => {
          if (node.localName === 't') return node.textContent;
          if (node.localName === 'tab') return '\t';
          if (node.localName === 'br') return <br key={n} />;
          const src = inlineImage(parts, related, node);
          return src ? <img key={n} src={src} alt="文档内嵌图片" loading="lazy" /> : null;
        })}
      </span>;
    });
    const align = first(block, 'jc');
    const value = align && localAttribute(align, 'val');
    const textAlign = value === 'center' || value === 'right' ? value : value === 'both' ? 'justify' : undefined;
    if (heading) return <h2 key={index} className={Number(heading) > 2 ? 'office-minor-heading' : undefined}
      style={{ textAlign }}>{runs}</h2>;
    return <p key={index} style={{ textAlign }}>{runs.length ? runs : '\u00a0'}</p>;
  }
  return <PreviewZoom className="preview-reading office-reading"><article className="office-word" aria-label="Word 正文">
    {children(body).map((part, i) => renderBlock(part, i))}
  </article></PreviewZoom>;
}

function columnNumber(address: string) {
  const letters = /^[A-Z]{1,3}/i.exec(address)?.[0].toUpperCase() ?? '';
  return [...letters].reduce((n, letter) => n * 26 + letter.charCodeAt(0) - 64, 0) - 1;
}
function columnName(index: number): string {
  const letter = String.fromCharCode(65 + index % 26);
  return index < 26 ? letter : columnName(Math.floor(index / 26) - 1) + letter;
}

function SheetPage({ parts }: { parts: OfficeParts }) {
  const book = 'xl/workbook.xml';
  const sheets = descendants(xml(parts, book), 'sheet');
  const related = useMemo(() => relations(parts, book), [parts]);
  const shared = useMemo(() => descendants(xml(parts, 'xl/sharedStrings.xml'), 'si').map(textOf), [parts]);
  const [sheetIndex, setSheetIndex] = useState(0);
  const [page, setPage] = useState(0);
  const sheet = sheets[sheetIndex];
  const path = sheet && related.get(relationshipId(sheet))?.path;
  const grid = useMemo(() => {
    if (!path) return undefined;
    const data = first(xml(parts, path), 'sheetData');
    if (!data) return undefined;
    const rows = children(data, 'row');
    let width = 1;
    for (const row of rows.slice(0, 10000)) {
      for (const cell of children(row, 'c')) width = Math.max(width, columnNumber(cell.getAttribute('r') ?? '') + 1);
    }
    return { rows: rows.slice(0, 10000), width: Math.min(width, 128), truncated: rows.length > 10000 || width > 128 };
  }, [parts, path]);
  if (!sheet || !grid) return <p className="preview-state" role="alert">工作表不可用，请下载后打开。</p>;
  const cells = Array.from({ length: grid.width }, (_, column) => column);
  const totalPages = Math.max(1, Math.ceil(grid.rows.length / 100));
  return <>
    <div className="document-controls">
      <label>工作表 <select value={sheetIndex} onChange={(event) => { setSheetIndex(Number(event.target.value)); setPage(0); }}>
        {sheets.map((item, index) => <option key={index} value={index}>{item.getAttribute('name') ?? '工作表 ' + (index + 1)}</option>)}
      </select></label>
      {totalPages > 1 && <Pager page={page} count={totalPages} onPage={setPage} />}
      {grid.truncated && <span>显示前 10,000 行、128 列，完整内容请下载查看。</span>}
    </div>
    <PreviewZoom className="office-sheet-scroll" role="region" aria-label="工作表内容" tabIndex={0} key={sheetIndex + ':' + page}>
      <table className="office-sheet">
        <thead><tr><th aria-label="行号" />{cells.map((c) => <th key={c} scope="col">{columnName(c)}</th>)}</tr></thead>
        <tbody>{grid.rows.slice(page * 100, (page + 1) * 100).map((row, index) => {
          const values = new Map<number, string>();
          for (const cell of children(row, 'c')) {
            const column = columnNumber(cell.getAttribute('r') ?? '');
            const kind = cell.getAttribute('t');
            const value = first(cell, 'v')?.textContent ?? '';
            if (column >= 0 && column < grid.width) values.set(column,
              kind === 's' ? shared[Number(value)] ?? '' : kind === 'inlineStr' ? textOf(cell)
                : kind === 'b' ? value === '1' ? 'TRUE' : 'FALSE' : value);
          }
          return <tr key={index}><th scope="row">{row.getAttribute('r') ?? page * 100 + index + 1}</th>
            {cells.map((c) => <td key={c} title={values.get(c)}>{values.get(c)}</td>)}</tr>;
        })}</tbody>
      </table>
    </PreviewZoom>
  </>;
}

function Pager({ page, count, onPage }: { page: number; count: number; onPage: (page: number) => void }) {
  return <div className="document-pager">
    <button type="button" className="icon-button" aria-label="上一页" disabled={page === 0}
      onClick={() => onPage(page - 1)}><ChevronLeft size={19} /></button>
    <span aria-live="polite">{page + 1} / {count}</span>
    <button type="button" className="icon-button" aria-label="下一页" disabled={page + 1 >= count}
      onClick={() => onPage(page + 1)}><ChevronRight size={19} /></button>
  </div>;
}

function SlidesPage({ parts }: { parts: OfficeParts }) {
  const main = 'ppt/presentation.xml';
  const presentation = xml(parts, main);
  const related = relations(parts, main);
  const slides = descendants(presentation, 'sldId')
    .map((node) => related.get(relationshipId(node))?.path).filter((v): v is string => !!v);
  const [page, setPage] = useState(0);
  const path = slides[page];
  const slide = path ? xml(parts, path) : undefined;
  if (!path || !slide) return <p className="preview-state" role="alert">幻灯片不可用，请下载后打开。</p>;
  const dimension = first(presentation, 'sldSz');
  const width = Number(dimension?.getAttribute('cx')) || 9144000;
  const height = Number(dimension?.getAttribute('cy')) || 6858000;
  const links = relations(parts, path);
  const layoutPath = [...links.values()].find((link) => link.type.endsWith('/slideLayout'))?.path;
  const layout = layoutPath ? xml(parts, layoutPath) : undefined;
  const shapes = children(first(slide, 'spTree')).filter((shape) => ['sp', 'pic', 'graphicFrame'].includes(shape.localName));
  return <>
    <div className="document-controls"><Pager page={page} count={slides.length} onPage={setPage} /></div>
    <div className="office-slide-scroll" role="region" aria-label="幻灯片内容" tabIndex={0}>
      <article className="office-slide" aria-label={'幻灯片 ' + (page + 1)}
        style={{ aspectRatio: Math.max(0.2, Math.min(5, width / height)) }}>
        {shapes.map((shape, i) => {
          const place = first(shape, 'ph');
          const fallback = place && descendants(layout, 'sp').find((item) => {
            const ph = first(item, 'ph');
            return ph && ph.getAttribute('idx') === place.getAttribute('idx') &&
              (ph.getAttribute('type') ?? 'body') === (place.getAttribute('type') ?? 'body');
          });
          const transform = first(shape, 'xfrm') ?? first(fallback || undefined, 'xfrm');
          const offset = first(transform, 'off');
          const extent = first(transform, 'ext');
          const x = offset ? Number(offset.getAttribute('x')) / width * 100 : 7;
          const y = offset ? Number(offset.getAttribute('y')) / height * 100 : (i === 0 ? 6 : 26);
          const w = extent ? Number(extent.getAttribute('cx')) / width * 100 : 86;
          const h = extent ? Number(extent.getAttribute('cy')) / height * 100 : (i === 0 ? 18 : 62);
          if (![x, y, w, h].every(Number.isFinite) || w <= 0 || h <= 0) return null;
          const image = inlineImage(parts, links, shape);
          const paragraphs = descendants(shape, 'p');
          if (!image && !paragraphs.length) return null;
          return <div className="office-slide-shape" key={i}
            style={{ left: x + '%', top: y + '%', width: w + '%', height: h + '%' }}>
            {image ? <img src={image} alt="幻灯片内嵌图片" /> : paragraphs.map((paragraph, n) => {
              const properties = first(paragraph, 'rPr');
              const pointSize = Number(properties?.getAttribute('sz')) / 100 || (i === 0 ? 28 : 17);
              return <p key={n} style={{ fontSize: Math.min(12, Math.max(1, pointSize * 12700 / width * 100)) + 'cqw',
                fontWeight: properties?.getAttribute('b') === '1' ? 650 : undefined }}>{textOf(paragraph)}</p>;
            })}
          </div>;
        })}
      </article>
    </div>
  </>;
}

export function OfficePreview({ source }: { source: PreviewSource }) {
  const [parts, setParts] = useState<OfficeParts>();
  const [error, setError] = useState('');
  const request = useMemo(() => ({ pending: undefined as Promise<OfficeParts> | undefined }), [source]);
  useEffect(() => {
    let cancelled = false;
    request.pending ??= contentPreviewApi.office(source);
    void request.pending.then((result) => {
      if (!cancelled) {
        if (result.xml['word/document.xml'] && !first(xml(result, 'word/document.xml'), 'body')) {
          setError('Word 正文不可用，请下载后打开。');
        } else setParts(result);
      }
    }, (reason) => { if (!cancelled) setError(errorText(reason)); });
    return () => { cancelled = true; };
  }, [request, source]);
  if (error) return <p className="preview-state" role="alert"><Info size={19} />{error}</p>;
  if (!parts) return <p className="preview-state" role="status"><Loader2 size={18} className="spin" />正在读取 Office 文档…</p>;
  if (parts.xml['word/document.xml']) return <WordPage parts={parts} />;
  if (parts.xml['xl/workbook.xml']) return <SheetPage parts={parts} />;
  if (parts.xml['ppt/presentation.xml']) return <SlidesPage parts={parts} />;
  return <p className="preview-state" role="alert">Office 文档内容不可用。</p>;
}
