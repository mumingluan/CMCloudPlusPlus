export type Space = 'personal_new' | 'group' | 'share';
export interface CloudFile {
  id: string;
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modifiedAt: string;
  thumbnail?: string | null;
}
export interface FilePage {
  files: CloudFile[];
  nextCursor?: string | null;
}
export interface AccountSummary {
  id: string;
  name: string;
  account: string;
  space: Space;
  rootId: string;
  cloudId: string;
  userDomainId: string;
  connected: boolean;
}
export interface LoginConfig {
  id: string;
  name: string;
  authorization: string;
  username: string;
  password: string;
  smsCode: string;
  mailCookies: string;
  space: Space;
  rootId: string;
  cloudId: string;
  userDomainId: string;
  linkId: string;
  customUploadPartSize: number;
  reportRealSize: boolean;
  useLargeThumbnail: boolean;
  useOldStreamUpload: boolean;
}
export interface TransferJob {
  id: string;
  name: string;
  direction: 'upload' | 'download';
  status: string;
  size: number;
  completed: number;
  speed: number;
  error?: string | null;
  localPath: string;
  createdAt: string;
}
export interface TransferSpec {
  direction: 'upload' | 'download';
  remote: CloudFile;
  localPath: string;
}
export interface Settings {
  concurrency: number;
  uploadPartSize: number;
  largeThumbnail: boolean;
  reportRealSize: boolean;
  oldStreamUpload: boolean;
}
export const spaceNames: Record<Space, string> = {
  personal_new: '个人云',
  group: '群组云',
  share: '分享链接',
};
export const initialConfig: LoginConfig = {
  id: '',
  name: '我的移动云盘',
  authorization: '',
  username: '',
  password: '',
  smsCode: '',
  mailCookies: '',
  space: 'personal_new',
  rootId: '',
  cloudId: '',
  userDomainId: '',
  linkId: '',
  customUploadPartSize: 0,
  reportRealSize: true,
  useLargeThumbnail: false,
  useOldStreamUpload: false,
};
