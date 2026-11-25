// @vitest-environment jsdom
import { createRef } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CryptCreateDialog, CryptWorkspace, type CryptControls } from './CryptWorkspace';
import { defaultCryptOptions, type CryptView } from './cryptApi';
import { cryptFile, cryptOperations, cryptTrail } from './cryptFiles';

const api = vi.hoisted(() => ({
  probe: vi.fn(),
  create: vi.fn(),
  open: vi.fn(),
  close: vi.fn(),
  list: vi.fn(),
  edit: vi.fn(),
  batchEdit: vi.fn(),
  updateSettings: vi.fn(),
  queueUploads: vi.fn(),
  queueUploadDirectory: vi.fn(),
  queueDownloads: vi.fn(),
  startPreview: vi.fn(),
  releasePreview: vi.fn(),
  openMediaPlayer: vi.fn(),
}));

vi.mock('./cryptApi', async () => {
  const actual = await vi.importActual<typeof import('./cryptApi')>('./cryptApi');
  return {
    ...actual,
    cryptApi: api,
    cryptDemo: false,
  };
});
vi.mock('./api', () => ({ errorText: String, native: false, normalizeJob: (x: unknown) => x }));

const folder = {
  id: 'crypt-folder-1',
  name: '私密备份',
  path: '/私密备份',
  isDir: true,
  size: 0,
  modifiedAt: '',
};

const baseView: CryptView = {
  handle: 'crypt-handle-1',
  folderId: 'crypt-folder-1',
  directory: '',
  items: [
    {
      name: '账单.pdf',
      path: '账单.pdf',
      kind: 'file',
      size: 2048,
      encryptedSize: 2096,
      modifiedAt: '2026-11-24T12:00:00+08:00',
    },
  ],
  saved: false,
  unrecognizedCount: 0,
  options: { ...defaultCryptOptions },
};

beforeEach(() => {
  vi.resetAllMocks();
  api.open.mockResolvedValue(baseView);
  api.create.mockResolvedValue(baseView);
  api.list.mockResolvedValue(baseView);
  api.close.mockResolvedValue(undefined);
  api.edit.mockResolvedValue(baseView);
  api.batchEdit.mockResolvedValue(baseView);
  api.updateSettings.mockResolvedValue({
    folderId: folder.id,
    known: false,
    saved: false,
    options: null,
  });
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});

afterEach(cleanup);

describe('OpenList CryptWorkspace & helpers', () => {
  it('creates an OpenList-compatible encrypted folder and accepts ___Obfuscated___ secrets without confirmation', async () => {
    const onCreated = vi.fn();
    render(<CryptCreateDialog parent={folder} onClose={() => {}} onCreated={onCreated} />);

    fireEvent.change(screen.getByLabelText('文件夹名称'), { target: { value: 'OpenList同步' } });
    fireEvent.change(screen.getByLabelText(/加密密码/), {
      target: { value: '___Obfuscated___abc123' },
    });
    expect(screen.queryByLabelText('确认密码')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /OpenList 兼容参数/ }));
    fireEvent.change(screen.getByLabelText(/文件名编码格式/), {
      target: { value: 'base32768' },
    });
    fireEvent.click(screen.getByRole('button', { name: /创建加密文件夹/ }));

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(api.create).toHaveBeenCalledWith(
      folder,
      'OpenList同步',
      '___Obfuscated___abc123',
      '',
      expect.objectContaining({
        filenameEncryption: 'standard',
        filenameEncoding: 'base32768',
        directoryNameEncryption: true,
      }),
      false,
    );
    expect(onCreated).toHaveBeenCalledWith(baseView, 'OpenList同步');
  });

  it('auto-unlocks when saved credentials exist and supports unbinding from management dialog', async () => {
    const onView = vi.fn();
    const onUnbound = vi.fn();
    const onClose = vi.fn();
    const controls = createRef<CryptControls>();

    render(
      <CryptWorkspace
        folder={folder}
        probe={{
          folderId: folder.id,
          known: true,
          saved: true,
          options: { ...defaultCryptOptions },
        }}
        controls={controls}
        onView={onView}
        onUnbound={onUnbound}
        onClose={onClose}
        onChanged={() => {}}
      />,
    );

    await waitFor(() =>
      expect(api.open).toHaveBeenCalledWith(folder, {
        options: defaultCryptOptions,
        useSaved: true,
        remember: true,
      }),
    );
    await waitFor(() => expect(onView).toHaveBeenCalledWith(baseView));

    act(() => controls.current?.manage());
    const unbind = await screen.findByRole('button', { name: /解除加密文件夹关联/ });
    fireEvent.click(unbind);

    await waitFor(() =>
      expect(api.updateSettings).toHaveBeenCalledWith(folder.id, {
        remember: false,
        forget: true,
      }),
    );
    expect(onUnbound).toHaveBeenCalledWith(folder.id);
    expect(onClose).toHaveBeenCalled();
  });

  it('maps cryptFile, cryptTrail, and cryptOperations cleanly', async () => {
    const mapped = cryptFile(folder.id, baseView.items[0]);
    expect(mapped.id).toBe('crypt:crypt-folder-1:账单.pdf');
    expect(mapped.size).toBe(2048);

    const trail = cryptTrail(folder.id, '2026/11月', '私密备份');
    expect(trail.map((t) => ({ name: t.name, path: t.path }))).toEqual([
      { name: '私密备份', path: '' },
      { name: '2026', path: '2026' },
      { name: '11月', path: '2026/11月' },
    ]);

    const accepted = vi.fn();
    const ops = cryptOperations(baseView, accepted);
    await ops.mkdir(trail[0], '子目录');
    expect(api.edit).toHaveBeenCalledWith('crypt-handle-1', 'mkdir', '子目录');
    expect(accepted).toHaveBeenCalledWith(baseView);
  });
});
