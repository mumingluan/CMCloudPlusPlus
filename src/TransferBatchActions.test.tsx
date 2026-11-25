// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { TransferBatchActions, BatchCancelDialog, batchCounts } from './TransferBatchActions';

afterEach(cleanup);
const summary = [
  { direction: 'upload', status: 'running', count: 450 },
  { direction: 'upload', status: 'paused', count: 2 },
  { direction: 'upload', status: 'completed', count: 100 },
  { direction: 'download', status: 'failed', count: 3 },
];
it('labels and scopes actions by category with summary counts covering all pages', () => {
  const onAction = vi.fn();
  const props = { summary, busy: false, connected: true, onAction };
  const app = render(<TransferBatchActions {...props} filter="active" />);
  fireEvent.click(screen.getByRole('button', { name: '暂停全部' }));
  expect(onAction).toHaveBeenLastCalledWith('pause', 'all');
  app.rerender(<TransferBatchActions {...props} filter="upload_active" />);
  fireEvent.click(screen.getByRole('button', { name: '开始上传' }));
  expect(onAction).toHaveBeenLastCalledWith('resume', 'upload');
  expect(batchCounts(summary, 'upload').cancel).toBe(452);
  app.rerender(<TransferBatchActions {...props} filter="download_active" />);
  expect((screen.getByRole('button', { name: '暂停下载' }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  fireEvent.click(screen.getByRole('button', { name: '取消下载' }));
  expect(onAction).toHaveBeenLastCalledWith('cancel', 'download');
});

it('disables every batch action when only completed or cancelled history remains', () => {
  render(
    <TransferBatchActions
      filter="active"
      summary={[
        { direction: 'upload', status: 'completed', count: 2 },
        { direction: 'download', status: 'cancelled', count: 4 },
      ]}
      busy={false}
      connected
      onAction={vi.fn()}
    />,
  );
  for (const button of screen.getAllByRole('button'))
    expect((button as HTMLButtonElement).disabled).toBe(true);
});

it('requires a second explicit cancel action and initially focuses keeping the tasks', () => {
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
  const onConfirm = vi.fn(),
    onClose = vi.fn();
  render(
    <BatchCancelDialog
      direction="download"
      count={450}
      busy={false}
      onClose={onClose}
      onConfirm={onConfirm}
    />,
  );
  expect(onConfirm).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: '保留任务' }));
  expect(screen.getByText(/450 个未完成的下载/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '确认取消下载' }));
  expect(onConfirm).toHaveBeenCalledTimes(1);
});
