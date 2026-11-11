import { describe, expect, it } from 'vitest';

import { formatDuration, formatSize } from './format';

describe('formatSize', () => {
  it('picks a unit and keeps three digits', () => {
    expect(formatSize(0)).toBe('0 B');
    expect(formatSize(1023)).toBe('1023 B');
    expect(formatSize(1536)).toBe('1.50 KB');
    expect(formatSize(20 * 1024 * 1024)).toBe('20.0 MB');
    expect(formatSize(300 * 1024 ** 3)).toBe('300 GB');
  });
});

describe('formatDuration', () => {
  it('minutes and hours', () => {
    expect(formatDuration(187_000)).toBe('3:07');
    expect(formatDuration(3_765_000)).toBe('1:02:45');
    expect(formatDuration(5_600)).toBe('0:05');
  });
});
