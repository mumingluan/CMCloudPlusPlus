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
export function readAppearance(): { theme: Theme; oled: boolean } {
  const stored = readPreference('cmcloud.theme', 'light');
  const oledPreference = readPreference('cmcloud.oledPureBlack', '');
  const legacyOled =
    oledPreference === '' && readPreference('cmcloud.colorScheme', 'classic') === 'oled';
  const theme: Theme = legacyOled
    ? 'dark'
    : stored === 'dark' || stored === 'system'
      ? stored
      : 'light';
  return { theme, oled: oledPreference === '' ? legacyOled : oledPreference === 'true' };
}
export function applyTheme(theme: Theme, oled = false) {
  const root = document.documentElement;
  const dark =
    theme === 'dark' ||
    (theme === 'system' &&
      typeof matchMedia === 'function' &&
      matchMedia('(prefers-color-scheme: dark)').matches);
  const pureBlack = dark && oled;
  root.dataset.colorScheme = pureBlack ? 'oled' : 'classic';
  root.dataset.theme = dark ? 'dark' : 'light';
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute('content', pureBlack ? '#000000' : dark ? '#141e2e' : '#ffffff');
}
const initial = readAppearance();
applyTheme(initial.theme, initial.oled);
