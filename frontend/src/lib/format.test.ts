import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration, formatNumber, percent, stripMinecraftColors } from './format';

describe('format', () => {
  it('formats bytes with sensible precision', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toMatch(/^1\.50 K/);
    expect(formatBytes(6.3 * 1024 ** 3, 1)).toMatch(/^6\.3 G/);
  });

  it('formats numbers and durations', () => {
    expect(formatNumber(1234567.4)).toBe('1,234,567');
    expect(formatDuration(42)).toBe('42s');
    expect(formatDuration(4 * 3600 + 12 * 60)).toBe('4h 12m');
  });

  it('clamps percentages', () => {
    expect(percent(5, 10)).toBe(50);
    expect(percent(5, 0)).toBe(0);
  });

  it('strips colour codes', () => {
    expect(stripMinecraftColors('§aHello §lworld')).toBe('Hello world');
  });
});
