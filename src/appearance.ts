export type Theme = 'system' | 'light' | 'dark';
export function readPreference(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}
export function savePreference(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Private browser storage may be unavailable. */
  }
}
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme =
    theme === 'system'
      ? typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : theme;
}
const stored = readPreference('cmcloud.theme', 'system');
applyTheme(['system', 'light', 'dark'].includes(stored) ? (stored as Theme) : 'system');
