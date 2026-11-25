import { useEffect, useRef } from 'react';
import { Pause, Play, X } from 'lucide-react';
import type { TransferFilter } from './transferView';

export type BatchDirection = 'all' | 'upload' | 'download';
export type BatchAction = 'pause' | 'resume' | 'cancel';
export type TransferCount = { direction?: string; status: string; count: number };
export const batchDirection = (filter: TransferFilter): BatchDirection =>
  filter.startsWith('upload_') ? 'upload' : filter.startsWith('download_') ? 'download' : 'all';
export const batchName = (direction: BatchDirection) =>
  direction === 'upload' ? '上传' : direction === 'download' ? '下载' : '全部';
export function batchCounts(summary: TransferCount[], direction: BatchDirection) {
  return summary.reduce(
    (value, entry) => {
      if (direction !== 'all' && entry.direction !== direction) return value;
      if (['running', 'queued', 'hashing', 'uploading', 'downloading'].includes(entry.status))
        value.pause += entry.count;
      if (['paused', 'failed'].includes(entry.status)) value.resume += entry.count;
      if (!['completed', 'cancelled'].includes(entry.status)) value.cancel += entry.count;
      return value;
    },
    { pause: 0, resume: 0, cancel: 0 },
  );
}

export function TransferBatchActions({
  filter,
  summary,
  busy,
  connected,
  onAction,
}: {
  filter: TransferFilter;
  summary: TransferCount[];
  busy: boolean;
  connected: boolean;
  onAction: (action: BatchAction, direction: BatchDirection) => void;
}) {
  const direction = batchDirection(filter),
    name = batchName(direction);
  const counts = batchCounts(summary, direction);
  return (
    <div className="transfer-batch-actions" role="group" aria-label="批量传输操作">
      <button
        className="button quiet"
        disabled={busy || counts.pause === 0}
        onClick={() => onAction('pause', direction)}
      >
        <Pause size={16} />
        暂停{name}
      </button>
      <button
        className="button quiet"
        disabled={busy || !connected || counts.resume === 0}
        onClick={() => onAction('resume', direction)}
      >
        <Play size={16} />
        开始{name}
      </button>
      <button
        className="button quiet danger"
        disabled={busy || counts.cancel === 0}
        onClick={() => onAction('cancel', direction)}
      >
        <X size={16} />
        取消{name}
      </button>
    </div>
  );
}

export function BatchCancelDialog({
  direction,
  count,
  busy,
  onClose,
  onConfirm,
}: {
  direction: BatchDirection;
  count: number;
  busy: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const keep = useRef<HTMLButtonElement>(null);
  const name = batchName(direction);
  useEffect(() => {
    dialog.current?.showModal();
    keep.current?.focus();
    return () => dialog.current?.close();
  }, []);
  return (
    <dialog
      className="operation-dialog batch-cancel-dialog"
      ref={dialog}
      aria-labelledby="batch-cancel-title"
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <div className="dialog-heading">
        <h2 id="batch-cancel-title">取消{name}？</h2>
      </div>
      <p>
        将取消 {count.toLocaleString()} 个未完成的{direction === 'all' ? '传输' : name}
        任务，包括其他分页中的任务。
      </p>
      <p>
        {direction === 'upload'
          ? '本地源文件和已完成的文件会保留。'
          : '未完成的下载断点会清理，已完成的文件会保留。'}
      </p>
      <div className="dialog-actions">
        <button ref={keep} className="button secondary" disabled={busy} onClick={onClose}>
          保留任务
        </button>
        <button className="button danger" disabled={busy || count === 0} onClick={onConfirm}>
          {busy ? '正在取消…' : `确认取消${name}`}
        </button>
      </div>
    </dialog>
  );
}
