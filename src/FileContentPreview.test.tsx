// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { FileContentPreview } from './FileContentPreview';

const requests = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: requests.invoke, isTauri: () => true }));

beforeEach(() => {
  requests.invoke.mockReset();
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(cleanup);

const file = { id: 'preview', path: '/readme.md', name: 'readme.md', isDir: false, size: 120, modifiedAt: '' };

it('renders Markdown as inert content and offers the exact source text', async () => {
  requests.invoke.mockResolvedValueOnce('# 文档\n\n**重点**\n\n<script>alert(1)</script>\n\n![远程图片](https://example.invalid/a.png)');
  render(<FileContentPreview file={file} kind="markdown" onClose={() => {}} onDownload={() => {}} />);
  expect(await screen.findByRole('heading', { name: '文档' })).toBeTruthy();
  expect(screen.getByText('重点').tagName).toBe('STRONG');
  expect(document.querySelector('article script, article img')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '源文本' }));
  expect(screen.getByText(/<script>alert/)).toBeTruthy();
});

it('offers download for oversized text without starting a read', () => {
  const download = vi.fn();
  render(<FileContentPreview file={{ ...file, size: 512 * 1024 + 1 }} kind="text" onClose={() => {}} onDownload={download} />);
  expect(screen.getByRole('alert').textContent).toContain('512 KB');
  expect(requests.invoke).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '下载文件' }));
  expect(download).toHaveBeenCalledTimes(1);
});

it('downloads a folder or mixed selection once and keeps the archive tree browseable', async () => {
  requests.invoke.mockImplementation(async (command: string) => command === 'open_archive_preview' ? {
    id: 'archive', format: 'ZIP', entries: [
      { id: 1, path: '手册/安装.txt', size: null, directory: false, downloadable: true },
      { id: 2, path: '说明.md', size: null, directory: false, downloadable: true },
    ],
  } : true);
  render(<FileContentPreview file={{ ...file, name: '资料.zip' }} kind="archive" onClose={() => {}} onDownload={() => {}} />);
  fireEvent.click(await screen.findByRole('button', { name: '手册 操作菜单' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '下载' }));
  await waitFor(() => expect(requests.invoke).toHaveBeenCalledWith('save_archive_selection', { id: 'archive', paths: ['手册'] }));
  await screen.findByText('所选内容已保存为 ZIP。');
  fireEvent.click(screen.getByRole('checkbox', { name: '选择 手册' }));
  fireEvent.click(screen.getByRole('checkbox', { name: '选择 说明.md' }));
  fireEvent.click(screen.getByRole('button', { name: '下载选中项' }));
  await waitFor(() => expect(requests.invoke).toHaveBeenCalledWith('save_archive_selection', { id: 'archive', paths: ['手册', '说明.md'] }));
  fireEvent.click(screen.getByRole('button', { name: '手册' }));
  fireEvent.click(screen.getByRole('button', { name: '安装.txt 操作菜单' }));
  expect(screen.getByRole('menuitem', { name: '下载' })).toBeTruthy();
  expect(screen.queryByRole('menuitem', { name: '删除' })).toBeNull();
  expect(screen.queryByRole('dialog')).toBeNull();
});

it('searches all descendants of the current archive directory and exposes whole-archive download in the toolbar', async () => {
  requests.invoke.mockResolvedValue({
    id: 'archive', format: 'ZIP', entries: [
      { id: 1, path: 'a/b/c.txt', size: 5, directory: false, downloadable: true },
      { id: 2, path: 'other/c.txt', size: 5, directory: false, downloadable: true },
    ],
  });
  const onDownload = vi.fn();
  render(<FileContentPreview file={{ ...file, name: '资料.zip' }} kind="archive"
    onClose={() => {}} onDownload={onDownload} />);
  expect(document.querySelector('.preview-topbar')).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '下载压缩包' }));
  expect(onDownload).toHaveBeenCalledTimes(1);
  fireEvent.click(await screen.findByRole('button', { name: 'a' }));
  expect(screen.queryByRole('button', { name: 'c.txt' })).toBeNull();
  fireEvent.change(screen.getByRole('searchbox', { name: '搜索' }), { target: { value: 'c.txt' } });
  const result = screen.getByRole('button', { name: 'c.txt' });
  expect(result.title).toBe('b/c.txt');
});

it('enters mobile archive selection from a long press without rendering the column headings', async () => {
  requests.invoke.mockResolvedValue({
    id: 'archive', format: 'ZIP', entries: [
      { id: 1, path: '说明.txt', size: 5, directory: false, downloadable: true },
      { id: 2, path: '数据.txt', size: 5, directory: false, downloadable: true },
    ],
  });
  render(<FileContentPreview file={{ ...file, name: '资料.zip' }} kind="archive" smallScreen
    onClose={() => {}} onDownload={() => {}} />);
  const entry = await screen.findByRole('button', { name: '说明.txt' });
  expect(screen.queryByRole('button', { name: '文件名称' })).toBeNull();
  fireEvent.contextMenu(entry);
  expect(entry.closest('.files-workspace')?.classList.contains('selecting-files')).toBe(true);
  expect(screen.getByRole('checkbox', { name: '选择 说明.txt' })).toHaveProperty('checked', true);
  fireEvent.click(screen.getByRole('checkbox', { name: '选择所有已加载文件' }));
  expect(screen.getByRole('checkbox', { name: '选择 数据.txt' })).toHaveProperty('checked', true);
  fireEvent.click(screen.getByRole('button', { name: '取消选择' }));
  expect(entry.closest('.files-workspace')?.classList.contains('selecting-files')).toBe(false);
});

it('shows Word text and ignores external document relationships', async () => {
  requests.invoke.mockResolvedValue({ images: {}, xml: {
    'word/document.xml': '<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Word 只读正文</w:t></w:r></w:p></w:body></w:document>',
  } });
  render(<FileContentPreview file={{ ...file, name: '文档.docx' }} kind="office" onClose={() => {}} onDownload={() => {}} />);
  expect(await screen.findByText('Word 只读正文')).toBeTruthy();
  expect(requests.invoke).toHaveBeenCalledWith('read_office_preview', expect.anything());
});

const rel = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
it('renders Excel cached values and switches worksheets', async () => {
  requests.invoke.mockResolvedValue({ images: {}, xml: {
    'xl/workbook.xml': '<workbook xmlns:r="' + rel + '"><sheets><sheet name="一月" r:id="r1"/><sheet name="二月" r:id="r2"/></sheets></workbook>',
    'xl/_rels/workbook.xml.rels': '<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Target="worksheets/sheet2.xml"/></Relationships>',
    'xl/worksheets/sheet1.xml': '<worksheet><sheetData><row r="1"><c r="A1"><f>6*7</f><v>42</v></c></row></sheetData></worksheet>',
    'xl/worksheets/sheet2.xml': '<worksheet><sheetData><row r="3"><c r="B3" t="inlineStr"><is><t>二月明细</t></is></c></row></sheetData></worksheet>',
  } });
  render(<FileContentPreview file={{ ...file, name: '台账.xlsx' }} kind="office" onClose={() => {}} onDownload={() => {}} />);
  expect(await screen.findByRole('cell', { name: '42' })).toBeTruthy();
  fireEvent.change(screen.getByRole('combobox', { name: '工作表' }), { target: { value: '1' } });
  expect(screen.getByRole('cell', { name: '二月明细' })).toBeTruthy();
});

it('uses the PowerPoint relationship ID and keeps the original slide order', async () => {
  requests.invoke.mockResolvedValue({ images: {}, xml: {
    'ppt/presentation.xml': '<presentation xmlns:r="' + rel + '"><sldIdLst><sldId id="256" r:id="s2"/><sldId id="257" r:id="s1"/></sldIdLst></presentation>',
    'ppt/_rels/presentation.xml.rels': '<Relationships><Relationship Id="s1" Target="slides/slide1.xml"/><Relationship Id="s2" Target="slides/slide2.xml"/></Relationships>',
    'ppt/slides/slide1.xml': '<sld><spTree><sp><p><t>结尾</t></p></sp></spTree></sld>',
    'ppt/slides/slide2.xml': '<sld><spTree><sp><p><t>开场</t></p></sp></spTree></sld>',
  } });
  render(<FileContentPreview file={{ ...file, name: '报告.pptx' }} kind="office" onClose={() => {}} onDownload={() => {}} />);
  expect(await screen.findByText('开场')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '下一页' }));
  expect(screen.getByText('结尾')).toBeTruthy();
});
