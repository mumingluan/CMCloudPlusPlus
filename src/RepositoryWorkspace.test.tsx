// @vitest-environment jsdom
import { StrictMode, createRef, useRef, useState } from 'react';
import type { ComponentProps } from 'react';
import type { RepositoryControls } from './RepositoryWorkspace';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { RepositoryWorkspace, RepositoryCreateDialog } from './RepositoryWorkspace';
import type { RepositoryView } from './repositoryApi';

const api = vi.hoisted(() => ({
  open: vi.fn(),
  close: vi.fn(),
  list: vi.fn(),
  plan: vi.fn(),
  execute: vi.fn(),
  edit: vi.fn(),
  download: vi.fn(),
  importKey: vi.fn(),
  forget: vi.fn(),
  createNamed: vi.fn(),
  generateKey: vi.fn(),
  changeUnlock: vi.fn(),
  labelCache: vi.fn(),
}));
vi.mock('./repositoryApi', () => ({ repositoryApi: api, repositoryDemo: false }));
vi.mock('./api', () => ({ errorText: String }));
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 64,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({ index, start: index * 64, size: 64 })),
  }),
}));
const folder = {
  id: 'folder',
  name: '归档',
  path: '/folder',
  isDir: true,
  size: 0,
  modifiedAt: '',
};
const probe = {
  folderId: 'folder',
  repositoryId: 'a'.repeat(32),
  kind: 'plain' as const,
  state: 'repository' as const,
  saved: false,
};
const view: RepositoryView = {
  handle: 'private-handle',
  repositoryId: 'a'.repeat(32),
  folderId: 'folder',
  revision: 'shown-revision',
  encrypted: false,
  directory: '',
  items: [
    {
      name: '测试.txt',
      path: '测试.txt',
      kind: 'file',
      size: 4,
      conflict: false,
      protected: false,
    },
  ],
  conflicts: [],
  historical: false,
  saved: false,
};
beforeEach(() => {
  vi.resetAllMocks();
  api.open.mockResolvedValue(view);
  api.list.mockResolvedValue(view);
  api.close.mockResolvedValue(undefined);
  api.labelCache.mockResolvedValue(undefined);
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
});
afterEach(cleanup);
it('opens management before loading and closes a session that finishes after cancellation', async () => {
  let finish!: (value: RepositoryView) => void;
  api.open.mockReturnValue(new Promise<RepositoryView>((resolve) => {
    finish = resolve;
  }));
  const browsing = vi.fn();
  const mounted = render(
    <RepositoryWorkspace
      folder={folder}
      probe={probe}
      manage
      onClose={() => mounted.unmount()}
      onView={browsing}
      onChanged={vi.fn()}
    />,
  );
  expect(screen.getByRole('dialog', { name: '管理对象库' })).toBeTruthy();
  expect(screen.getByRole('status').textContent).toContain('正在加载');
  fireEvent.click(screen.getByRole('button', { name: '取消' }));
  await act(async () => finish(view));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(api.close).toHaveBeenCalledExactlyOnceWith(view.handle);
  expect(browsing).not.toHaveBeenCalled();
});

it('guards creation modal dismissal while the native key picker is pending', async () => {
  let finish!: (value: { selectionId: string; name: string }) => void;
  api.generateKey.mockReturnValue(
    new Promise((r) => {
      finish = r;
    }),
  );
  const close = vi.fn();
  render(<RepositoryCreateDialog parent={folder} onClose={close} onCreated={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('对象库类型'), { target: { value: 'encrypted' } });
  fireEvent.click(screen.getByRole('button', { name: '密钥' }));
  fireEvent.click(screen.getByRole('button', { name: '生成并保存新密钥' }));
  fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(close).not.toHaveBeenCalled();
  await act(async () => finish({ selectionId: 'opaque-picker', name: 'new.key' }));
  fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(close).toHaveBeenCalledOnce();
});
it('guards management dismissal during key selection and restores Escape after it settles', async () => {
  const close = vi.fn();
  let finish!: (value: { selectionId: string; name: string }) => void;
  api.generateKey.mockReturnValue(
    new Promise((r) => {
      finish = r;
    }),
  );
  render(
    <RepositoryWorkspace
      folder={folder}
      initialView={{ ...view, encrypted: true, canChangeUnlock: true }}
      manage
      onClose={close}
      onChanged={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole('button', { name: '更换密钥' }));
  fireEvent.click(screen.getByRole('button', { name: '生成并保存新密钥' }));
  fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.getByRole('heading', { name: '更换对象库密钥' })).toBeTruthy();
  window.dispatchEvent(new Event('cmcloud-repository-back', { cancelable: true }));
  expect(screen.getByRole('dialog')).toBeTruthy();
  await act(async () => finish({ selectionId: 'opaque-picker', name: 'new.key' }));
  fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(close).toHaveBeenCalledOnce();
});
it('locks the submitted type and all credential controls until a delayed creation fails', async () => {
  let reject!: (e: Error) => void;
  api.createNamed.mockReturnValue(
    new Promise((_resolve, r) => {
      reject = r;
    }),
  );
  render(<RepositoryCreateDialog parent={folder} onClose={vi.fn()} onCreated={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('对象库名称'), { target: { value: '延迟创建' } });
  fireEvent.change(screen.getByLabelText('对象库类型'), { target: { value: 'encrypted' } });
  fireEvent.change(screen.getByLabelText('设置密码'), { target: { value: 'pending-password' } });
  fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'pending-password' } });
  fireEvent.click(screen.getByRole('button', { name: '创建对象库' }));
  for (const label of ['对象库名称', '对象库类型', '设置密码', '确认密码'])
    expect((screen.getByLabelText(label) as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole('checkbox') as HTMLInputElement).disabled).toBe(true);
  for (const name of ['密码', '密钥', '取消'])
    expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: '密钥' }));
  expect(screen.queryByLabelText('密钥')).toBeNull();
  await act(async () => reject(new Error('创建失败')));
  expect((await screen.findByRole('alert')).textContent).toContain('创建失败');
  expect((screen.getByLabelText('对象库类型') as HTMLSelectElement).value).toBe('encrypted');
  expect((screen.getByRole('button', { name: '密钥' }) as HTMLButtonElement).disabled).toBe(false);
});
it('keeps the password method stable during a delayed credential change and restores it on failure', async () => {
  let reject!: (e: Error) => void;
  api.changeUnlock.mockReturnValue(
    new Promise((_resolve, r) => {
      reject = r;
    }),
  );
  const encrypted = {
    ...view,
    encrypted: true,
    credentialRevision: 'credential-v1',
    unlockMethods: ['password'],
    canChangeUnlock: true,
  };
  render(
    <RepositoryWorkspace
      folder={folder}
      initialView={encrypted}
      manage
      onClose={vi.fn()}
      onChanged={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole('button', { name: '修改密码' }));
  fireEvent.change(screen.getByLabelText('对象库密码'), { target: { value: 'replacement' } });
  fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'replacement' } });
  fireEvent.click(screen.getByRole('button', { name: '保存解锁方式' }));
  expect((screen.getByLabelText('对象库密码') as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole('button', { name: '密钥' }) as HTMLButtonElement).disabled).toBe(true);
  expect((screen.getByRole('checkbox') as HTMLInputElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('button', { name: '密钥' }));
  expect(screen.getByLabelText('对象库密码')).toBeTruthy();
  await act(async () => reject(new Error('更新失败')));
  expect((await screen.findByRole('alert')).textContent).toContain('更新失败');
  expect((screen.getByRole('button', { name: '密钥' }) as HTMLButtonElement).disabled).toBe(false);
});
it('locks management actions during delayed saved-credential removal and restores them after failure', async () => {
  let reject!: (e: Error) => void;
  api.forget.mockReturnValue(
    new Promise((_resolve, r) => {
      reject = r;
    }),
  );
  const encrypted = { ...view, saved: true, encrypted: true, canChangeUnlock: true };
  render(
    <RepositoryWorkspace
      folder={folder}
      initialView={encrypted}
      manage
      onClose={vi.fn()}
      onChanged={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole('button', { name: '清除本机保存的解锁信息' }));
  for (const name of ['修改密码', '更换密钥', '完成', '清除本机保存的解锁信息'])
    expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => reject(new Error('清除失败')));
  expect((await screen.findByRole('alert')).textContent).toContain('清除失败');
  for (const name of ['修改密码', '更换密钥', '完成'])
    expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(false);
});
it('keeps a newly created session across StrictMode effect replay and closes it on real exit', async () => {
  const mounted = render(
    <StrictMode>
      <RepositoryWorkspace
        folder={folder}
        initialView={view}
        probe={probe}
        onClose={vi.fn()}
        onChanged={vi.fn()}
      />
    </StrictMode>,
  );
  await act(async () => {});
  expect(api.close).not.toHaveBeenCalled();
  mounted.unmount();
  await waitFor(() => expect(api.close).toHaveBeenCalledWith(view.handle));
});
it('lets the shared browser navigate a verified session and explicitly refresh on request', async () => {
  const controls=createRef<RepositoryControls>();
  api.list.mockResolvedValue({...view,directory:'资料'});
  render(<RepositoryWorkspace folder={folder} initialView={view} controls={controls} onClose={vi.fn()} onChanged={vi.fn()}/>);
  await act(async()=>{await controls.current!.refresh('资料',false)});
  expect(api.list).toHaveBeenCalledWith(view.handle,'资料',false);
  await act(async()=>{await controls.current!.refresh(undefined,true)});
  expect(api.list).toHaveBeenCalledWith(view.handle,'资料',true);
  expect(api.open).not.toHaveBeenCalled();
});
it('creates ordinary and encrypted repositories from the current parent and checks password confirmation', async () => {
  const created = vi.fn();
  api.createNamed.mockResolvedValue(view);
  render(<RepositoryCreateDialog parent={folder} onClose={vi.fn()} onCreated={created} />);
  fireEvent.change(screen.getByLabelText('对象库名称'), { target: { value: '个人资料' } });
  fireEvent.change(screen.getByLabelText('对象库类型'), { target: { value: 'encrypted' } });
  fireEvent.change(screen.getByLabelText('设置密码'), { target: { value: 'new-password' } });
  fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'different' } });
  fireEvent.click(screen.getByRole('button', { name: '创建对象库' }));
  expect((await screen.findByRole('alert')).textContent).toContain('两次输入');
  expect(api.createNamed).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'new-password' } });
  fireEvent.click(screen.getByRole('button', { name: '创建对象库' }));
  await waitFor(() =>
    expect(api.createNamed).toHaveBeenCalledWith(
      folder,
      '个人资料',
      true,
      { password: 'new-password' },
      false,
    ),
  );
  expect(created).toHaveBeenCalledWith(view, '个人资料');
});
it('keeps generated key material native and only submits its opaque selection', async () => {
  api.generateKey.mockResolvedValue({ selectionId: 'native-key-handle', name: 'AxuanStor.key' });
  api.createNamed.mockResolvedValue(view);
  render(<RepositoryCreateDialog parent={folder} onClose={vi.fn()} onCreated={vi.fn()} />);
  fireEvent.change(screen.getByLabelText('对象库名称'), { target: { value: '加密资料' } });
  fireEvent.change(screen.getByLabelText('对象库类型'), { target: { value: 'encrypted' } });
  fireEvent.click(screen.getByRole('button', { name: '密钥' }));
  fireEvent.click(screen.getByRole('button', { name: '生成并保存新密钥' }));
  await screen.findByRole('button', { name: 'AxuanStor.key' });
  fireEvent.click(screen.getByRole('button', { name: '创建对象库' }));
  await waitFor(() =>
    expect(api.createNamed).toHaveBeenCalledWith(
      folder,
      '加密资料',
      true,
      { key: '', keyFile: 'native-key-handle' },
      false,
    ),
  );
});
it('opens management from the repository menu and fences credential replacement with the displayed revision', async () => {
  const encrypted = {
    ...view,
    encrypted: true,
    credentialRevision: 'credential-v1',
    unlockMethods: ['password'],
    canChangeUnlock: true,
  };
  api.changeUnlock.mockResolvedValue({
    view: { ...encrypted, credentialRevision: 'credential-v2' },
    notice: null,
  });
  render(
    <RepositoryWorkspace
      folder={folder}
      initialView={encrypted}
      manage
      probe={{ ...probe, kind: 'encrypted' }}
      onClose={vi.fn()}
      onChanged={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole('button', { name: '修改密码' }));
  fireEvent.change(screen.getByLabelText('对象库密码'), { target: { value: 'replacement' } });
  fireEvent.change(screen.getByLabelText('确认密码'), { target: { value: 'replacement' } });
  fireEvent.click(screen.getByRole('button', { name: '保存解锁方式' }));
  await waitFor(() =>
    expect(api.changeUnlock).toHaveBeenCalledWith(
      view.handle,
      'credential-v1',
      { password: 'replacement' },
      false,
    ),
  );
  expect(await screen.findByRole('heading', { name: '管理对象库' })).toBeTruthy();
});
function UploadController(props:ComponentProps<typeof RepositoryWorkspace>){
  const controls=useRef<RepositoryControls>(null);const [ready,setReady]=useState(false);const [busy,setBusy]=useState(false);
  return <>{ready&&<button disabled={busy} onClick={()=>controls.current?.upload()}>上传文件</button>}<RepositoryWorkspace {...props} controls={controls} onView={view=>setReady(!!view)} onBusy={setBusy}/></>;
}
it('focuses an asynchronously prepared plan and restores the shared trigger after durable queue acceptance', async () => {
  let planned!:(value:unknown)=>void;let queued!:(value:unknown[])=>void;
  api.plan.mockReturnValue(new Promise(resolve=>{planned=resolve}));
  api.execute.mockReturnValue(new Promise(resolve=>{queued=resolve}));
  render(<UploadController folder={folder} probe={probe} onClose={vi.fn()} onChanged={vi.fn()}/>);
  const upload=await screen.findByRole('button',{name:'上传文件'});upload.focus();fireEvent.click(upload);
  await act(async()=>planned({id:'plan',items:[],logicalBytes:0,sourceBytes:0,uploadBytes:0,uniqueUploads:0}));
  await waitFor(()=>expect(document.activeElement).toBe(screen.getByRole('button',{name:'关闭上传比对'})));
  fireEvent.click(screen.getByRole('button',{name:'确认上传'}));
  expect(upload.hasAttribute('disabled')).toBe(true);
  await act(async()=>queued([]));
  await waitFor(()=>expect(screen.queryByRole('dialog')).toBeNull());
  await waitFor(()=>expect(document.activeElement).toBe(upload));
  expect(api.list).not.toHaveBeenCalled();
});
it('keeps remember opt-in and submits imported keys as opaque native selections', async () => {
  api.importKey.mockResolvedValue({ selectionId: 'opaque-selection', name: '私有.key' });
  render(
    <RepositoryWorkspace
      folder={folder}
      probe={{ ...probe, kind: 'encrypted' }}
      onClose={vi.fn()}
      onChanged={vi.fn()}
    />,
  );
  const remember = screen.getByRole('checkbox');
  expect((remember as HTMLInputElement).checked).toBe(false);
  fireEvent.click(screen.getByRole('button', { name: '密钥' }));
  fireEvent.click(screen.getByRole('button', { name: '导入密钥文件' }));
  await screen.findByRole('button', { name: '私有.key' });
  fireEvent.click(remember);
  fireEvent.click(screen.getByRole('button', { name: '解锁' }));
  await waitFor(() =>
    expect(api.open).toHaveBeenCalledWith(
      'folder',
      probe.repositoryId,
      { key: '', keyFile: 'opaque-selection' },
      false,
      true,
      expect.any(Function),
    ),
  );
});
it('requires explicit replacement confirmation and suppresses double submit while a batch is pending', async () => {
  let finish!: () => void;
  api.execute.mockReturnValue(
    new Promise<void>((resolve) => {
      finish = resolve;
    }),
  );
  api.plan.mockResolvedValue({
    id: 'confirmed-plan',
    items: [{ path: '测试.txt', size: 8, sha256: 'd'.repeat(64), action: 'replace-upload' }],
    logicalBytes: 8,
    sourceBytes: 8,
    uploadBytes: 8,
    uniqueUploads: 1,
  });
  render(
    <UploadController folder={folder} probe={probe} onClose={vi.fn()} onChanged={vi.fn()} />,
  );
  fireEvent.click(await screen.findByRole('button', { name: '上传文件' }));
  const submit = await screen.findByRole('button', { name: '确认上传' });
  expect((submit as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole('checkbox', { name: '确认替换所列的已有路径' }));
  fireEvent.click(submit);
  fireEvent.click(submit);
  expect(api.execute).toHaveBeenCalledTimes(1);
  expect(api.execute).toHaveBeenCalledWith('private-handle', 'confirmed-plan', true);
  await act(async () => finish());
  expect(screen.queryByRole('dialog')).toBeNull();
});
it('hands the original view and revision to the shared file browser', async () => {
  const onView=vi.fn();
  render(<RepositoryWorkspace folder={folder} probe={probe} onView={onView} onClose={vi.fn()} onChanged={vi.fn()}/>);
  await waitFor(()=>expect(onView).toHaveBeenCalledWith(view));
  expect(screen.queryByRole('table')).toBeNull();
  expect(screen.queryByRole('button',{name:'下载 测试.txt'})).toBeNull();
});
it('closes a delayed abandoned native session after StrictMode remounts', async () => {
  let abandoned!: (value: RepositoryView) => void;
  api.open
    .mockReturnValueOnce(
      new Promise<RepositoryView>((resolve) => {
        abandoned = resolve;
      }),
    )
    .mockResolvedValueOnce(view);
  const rendered = render(
    <StrictMode>
      <RepositoryWorkspace folder={folder} probe={probe} onClose={vi.fn()} onChanged={vi.fn()} />
    </StrictMode>,
  );
  await waitFor(()=>expect(api.labelCache).toHaveBeenCalledWith('private-handle',folder.name));
  await act(async () => abandoned({ ...view, handle: 'abandoned' }));
  expect(api.close).toHaveBeenCalledWith('abandoned');
  rendered.unmount();
  await waitFor(() => expect(api.close).toHaveBeenCalledWith('private-handle'));
});
