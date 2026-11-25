// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { applyTheme, readAppearance } from './appearance';
beforeEach(() => localStorage.clear());
afterEach(() => vi.unstubAllGlobals());
it('migrates the previous OLED scheme without changing its dark appearance', () => {
  localStorage.setItem('cmcloud.theme', 'light');
  localStorage.setItem('cmcloud.colorScheme', 'oled');
  expect(readAppearance()).toEqual({ theme: 'dark', oled: true });
});
it('keeps a saved light mode when OLED is enabled for future dark mode use', () => {
  localStorage.setItem('cmcloud.theme', 'light');
  localStorage.setItem('cmcloud.oledPureBlack', 'true');
  localStorage.setItem('cmcloud.colorScheme', 'oled');
  expect(readAppearance()).toEqual({ theme: 'light', oled: true });
  applyTheme('light', true);
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(document.documentElement.dataset.colorScheme).toBe('classic');
  applyTheme('dark', true);
  expect(document.documentElement.dataset.colorScheme).toBe('oled');
});
it('preserves a system preference and resolves its effective appearance', () => {
  localStorage.setItem('cmcloud.theme', 'system');
  vi.stubGlobal('matchMedia', () => ({ matches: true }));
  expect(readAppearance()).toEqual({ theme: 'system', oled: false });
  applyTheme('system', true);
  expect(document.documentElement.dataset.theme).toBe('dark');
  expect(document.documentElement.dataset.colorScheme).toBe('oled');
});
