import { ArrowDownToLine, ArrowUpFromLine } from 'lucide-react';
import { formatBytes } from './utils';
import { progressSegments } from './transferView';
import type { TransferFilter, TransferOverview as Overview } from './transferView';

export function TransferOverview({
  overview,
  filter,
}: {
  overview: Overview;
  filter: TransferFilter;
}) {
  const value = progressSegments(overview, filter);
  const title =
    filter === 'upload_active'
      ? '总上传进度'
      : filter === 'download_active'
        ? '总下载进度'
        : '总进度';
  return (
    <div className="transfer-overview" data-empty={!value.total && !overview.uploadSpeed && !overview.downloadSpeed || undefined}>
      <div className="transfer-overview-heading">
        <strong>{title}</strong>
        <span>
          {formatBytes(value.completed)} / {formatBytes(value.total)}
          <b>{value.percent.toFixed(1)}%</b>
        </span>
      </div>
      <div
        className="transfer-total-bar"
        role="progressbar"
        aria-label={title}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(value.percent)}
      >
        {value.segments.map((segment) => (
          <span
            key={segment.kind}
            className={segment.kind}
            style={{ width: `${(segment.bytes / value.total) * 100}%` }}
          />
        ))}
      </div>
      <div className="transfer-total-speeds">
        <span className="upload-speed">
          <ArrowUpFromLine size={15} />
          总上传速度 <strong>{formatBytes(overview.uploadSpeed)}/s</strong>
        </span>
        <span className="download-speed">
          <ArrowDownToLine size={15} />
          总下载速度 <strong>{formatBytes(overview.downloadSpeed)}/s</strong>
        </span>
      </div>
    </div>
  );
}
