import type { AccountSummary, CloudFile, TransferJob } from './types';
// Explicit browser-only, synthetic scale fixture. Never used in native sessions.
export function scalePreview(): {
  account: AccountSummary;
  files: CloudFile[];
  jobs: TransferJob[];
} {
  const folders = ['工作文档', '旅行影像', '家庭相册', '设计素材', '设备备份', '学习资料'];
  const names = ['项目计划', '西北旅行', '产品原型', '年度总结', '照片备份', '会议记录'];
  const exts = ['pdf', 'mp4', 'fig', 'docx', 'zip', 'xlsx'];
  const files: CloudFile[] = Array.from({ length: 10000 }, (_, i) => ({
    id: `fixture-${i}`,
    name: i < 6 ? folders[i] : `${names[i % 6]}_${String(i).padStart(5, '0')}.${exts[i % 6]}`,
    path: `/fixture-${i}`,
    isDir: i < 6,
    size: i < 6 ? 0 : i === 7 ? 2 * 1024 ** 4 : ((i + 11) * 172981) % 1024 ** 3,
    modifiedAt: new Date(Date.UTC(2026, 9, 1 + (i % 5), 9, i % 60)).toISOString(),
  }));
  const jobs: TransferJob[] = [
    {
      id: 'fixture-transfer-1',
      name: '照片与视频归档.zip',
      direction: 'upload',
      status: 'running',
      size: 2 * 1024 ** 4,
      completed: 1024 ** 4,
      speed: 0,
      localPath: '示例文件 · 用于界面预览',
      createdAt: '',
    },
    {
      id: 'fixture-transfer-2',
      name: '项目文档.pdf',
      direction: 'download',
      status: 'paused',
      size: 28 * 1024 ** 2,
      completed: 10 * 1024 ** 2,
      speed: 0,
      localPath: '示例文件 · 用于界面预览',
      createdAt: '',
    },
    {
      id: 'fixture-transfer-3',
      name: '家庭影像_2026_示例.zip',
      direction: 'download',
      status: 'failed',
      size: 32 * 1024 ** 3,
      completed: 12 * 1024 ** 3,
      speed: 0,
      localPath: 'D:\\示例下载\\家庭归档\\2026年完整影像与照片备份\\家庭影像_2026_示例.zip',
      error:
        '示例错误：本地磁盘空间不足，无法继续写入下载分片。请释放下载位置的空间，确认至少有 20 GB 可用容量，然后点击重试。已经下载并同步到磁盘的分片将保留。',
      createdAt: '',
    },
  ];
  return {
    account: {
      id: 'preview',
      name: '示例工作空间',
      account: '示例数据',
      space: 'personal_new',
      rootId: '/',
      cloudId: '',
      userDomainId: '',
      connected: false,
    },
    files,
    jobs,
  };
}
