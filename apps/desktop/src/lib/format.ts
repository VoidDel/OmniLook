// Sizes and dates for the preview's status line.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function formatSize(bytes: number): string {
  let v = bytes;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${v} ${UNITS[0]}` : `${v.toFixed(v < 10 ? 2 : v < 100 ? 1 : 0)} ${UNITS[u]}`;
}

export function formatDate(ms: number | null, lang: string): string {
  if (ms == null) return '';
  return new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(ms));
}
