import { fileViewLabels, type FileView } from './FileBrowserParts';
import { FileMenuDialog } from './FileMenuDialog';
import { ArrowDown, ArrowUp, Grid2X2, Grid3X3, Grip, List } from 'lucide-react';

const sortKeys = [['name', '名称'], ['modifiedAt', '修改时间'], ['size', '大小']] as const;
const sortDirections = [[false, '升序'], [true, '降序']] as const;
const viewIcons = { list: List, grid: Grid2X2, 'grid-medium': Grid3X3, 'grid-small': Grip };

export function FileViewOptions({
  view,
  setView,
  sort,
  setSort,
  filter,
  setFilter,
  onClose,
}: {
  view: FileView;
  setView: (value: FileView) => void;
  sort: { key: string; desc: boolean };
  setSort: (value: { key: string; desc: boolean }) => void;
  filter: string;
  setFilter: (value: string) => void;
  onClose: () => void;
}) {
  return (
    <FileMenuDialog title="文件显示选项" closeLabel="关闭显示选项" onClose={onClose}>
      <div className="file-view-options">
        <div className="file-view-modes file-view-segment" role="group" aria-label="显示方式">
          {(Object.keys(fileViewLabels) as FileView[]).map((mode) => {
            const Icon = viewIcons[mode];
            return (
              <button key={mode} aria-pressed={view === mode} onClick={() => setView(mode)}>
                <Icon size={19} aria-hidden="true" />
                {fileViewLabels[mode]}
              </button>
            );
          })}
        </div>
        <div className="file-view-sort" role="group" aria-label="排序方式">
          <span>排序方式</span>
          <div className="file-view-segment">
            {sortKeys.map(([key, label]) => (
              <button key={key} aria-pressed={sort.key === key} onClick={() => setSort({ ...sort, key })}>
                {label}
              </button>
            ))}
          </div>
        </div>
        <div className="file-view-sort" role="group" aria-label="排序方向">
          <span>排序方向</span>
          <div className="file-view-segment">
            {sortDirections.map(([desc, label]) => (
              <button key={label} aria-pressed={sort.desc === desc} onClick={() => setSort({ ...sort, desc })}>
                {desc ? <ArrowDown size={14} aria-hidden="true" /> : <ArrowUp size={14} aria-hidden="true" />}
                {label}
              </button>
            ))}
          </div>
        </div>
        <label>
          类型筛选
          <select
            aria-label="文件类型"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          >
            <option value="all">全部类型</option>
            <option value="folders">文件夹</option>
            <option value="图片">图片</option>
            <option value="视频">视频</option>
            <option value="音频">音频</option>
            <option value="PDF">PDF</option>
          </select>
        </label>
      </div>
    </FileMenuDialog>
  );
}
