import { describe, expect, it } from 'vitest';
import {
  formatBytes,
  formatCloudPath,
  formatRepositoryPath,
  parseAddressPath,
  sortFiles,
  validName,
} from './utils';
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
  it('formats cloud and object repository paths consistently', () => {
    expect(formatCloudPath([])).toBe('/');
    expect(formatCloudPath([{ name: 'objects' }, { name: 'x3-client' }])).toBe('/objects/x3-client');
    expect(formatRepositoryPath('/objects/x3-client', '')).toBe('/objects/x3-client:');
    expect(formatRepositoryPath('/objects/x3-client', '1.7.1300/Total')).toBe(
      '/objects/x3-client:1.7.1300/Total',
    );
  });
  it('parses cloud, relative, and object repository address bar paths', () => {
    expect(parseAddressPath('/')).toEqual({ kind: 'cloud', segments: [] });
    expect(parseAddressPath('  \\objects\\x3-client\\ ')).toEqual({
      kind: 'cloud',
      segments: ['objects', 'x3-client'],
    });
    expect(parseAddressPath('/objects/a/../x3-client:1.7.1300/Total/./Audio/..')).toEqual({
      kind: 'repository',
      cloudSegments: ['objects', 'x3-client'],
      innerPath: '1.7.1300/Total',
    });
    expect(parseAddressPath('/objects/x3-client:')).toEqual({
      kind: 'repository',
      cloudSegments: ['objects', 'x3-client'],
      innerPath: '',
    });
    expect(
      parseAddressPath('Total/Audio', {
        cloudSegments: ['objects'],
        repository: { cloudSegments: ['objects', 'x3-client'], innerPath: '1.7.1300' },
      }),
    ).toEqual({
      kind: 'repository',
      cloudSegments: ['objects', 'x3-client'],
      innerPath: '1.7.1300/Total/Audio',
    });
    expect(
      parseAddressPath('/objects', {
        cloudSegments: ['objects'],
        repository: { cloudSegments: ['objects', 'x3-client'], innerPath: '1.7.1300/Total' },
      }),
    ).toEqual({
      kind: 'cloud',
      segments: ['objects'],
    });
  });
});
