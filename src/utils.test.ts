import { describe, expect, it } from 'vitest';
import { formatBytes, sortFiles, validName } from './utils';
import { scalePreview } from './preview';
describe('large-file and directory view invariants', () => {
  it('formats TB sizes without truncating to 32 bits', () => {
    expect(formatBytes(2 * 1024 ** 4)).toBe('2 TB');
    expect(formatBytes(5 * 1024 ** 4)).toBe('5 TB');
  });
  it('sorts 10,000 entries without losing files and keeps folders first', () => {
    const files = scalePreview().files;
    const sorted = sortFiles(files, 'size', true);
    expect(sorted).toHaveLength(10000);
    expect(new Set(sorted.map((f) => f.id)).size).toBe(10000);
    expect(sorted.slice(0, 6).every((f) => f.isDir)).toBe(true);
    expect(sorted[6].size).toBe(2 * 1024 ** 4);
  });
  it('rejects traversal and control characters in cloud names', () => {
    for (const name of ['..', '.', '../x', 'x/y', 'x\\y', 'x\0y', ' '])
      expect(validName(name)).toBe(false);
    expect(validName('2026 项目文档.pdf')).toBe(true);
  });
});
