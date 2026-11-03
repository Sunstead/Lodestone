/** Number, byte and time formatting shared by every page. */

const KB = 1024;
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

export function formatBytes(bytes: number, decimals?: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(KB)), UNITS.length - 1);
  const value = bytes / KB ** exponent;
  const digits = decimals ?? (value >= 100 || exponent === 0 ? 0 : value >= 10 ? 1 : 2);
  return `${value.toFixed(digits)} ${UNITS[exponent]}`;
}

export function formatNumber(value: number): string {
  return new Intl.NumberFormat('en-US').format(Math.round(value));
}

/** Seconds -> `3d 14h 22m`, `4h 12m`, `12m 30s`, `42s`. */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return '—';
  const seconds = Math.floor(totalSeconds % 60);
  const minutes = Math.floor((totalSeconds / 60) % 60);
  const hours = Math.floor((totalSeconds / 3600) % 24);
  const days = Math.floor(totalSeconds / 86400);
  if (days > 0) return `${days}d ${hours}h ${minutes}m`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

/** Milliseconds since epoch -> `14:32:08` (console timestamps). */
export function formatClock(ts: number): string {
  const d = new Date(ts);
  const pad = (n: number) => n.toString().padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** Milliseconds since epoch -> `Oct 17, 06:41`. */
export function formatDateTime(ts: number): string {
  if (!ts) return '—';
  return new Date(ts).toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
}

/** ISO string / epoch ms -> `Today, 12:00`, `Yesterday, 21:14`, `Oct 14, 09:03`. */
export function formatWhen(value: string | number | null | undefined): string {
  if (!value) return '—';
  const ts = typeof value === 'number' ? value : Date.parse(value);
  if (!Number.isFinite(ts)) return '—';
  const date = new Date(ts);
  const today = new Date();
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
  const time = date.toLocaleTimeString('en-US', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (sameDay(date, today)) return `Today, ${time}`;
  if (sameDay(date, yesterday)) return `Yesterday, ${time}`;
  return date.toLocaleString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false });
}

/** Seconds since epoch-relative timestamp -> `just now`, `2 hours ago`, `Oct 12`. */
export function formatRelative(ts: number | string | null | undefined): string {
  if (!ts) return 'never';
  const time = typeof ts === 'number' ? ts : Date.parse(ts);
  if (!Number.isFinite(time)) return 'never';
  const diff = Math.max(0, Date.now() - time) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}d ago`;
  return formatWhen(time);
}

/** Seconds -> `2h 14m` for player sessions. */
export function formatPlaytime(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${minutes.toString().padStart(2, '0')}m`;
  if (minutes > 0) return `${minutes}m`;
  return `${Math.floor(totalSeconds % 60)}s`;
}

export function formatPing(ms: number | null): string {
  return ms === null ? '—' : `${ms} ms`;
}

export function percent(used: number, total: number): number {
  if (!total) return 0;
  return Math.min(100, Math.max(0, (used / total) * 100));
}

export const pingTone = (ms: number | null): string => {
  if (ms === null) return 'text-muted-foreground';
  return ms < 50 ? 'text-green-500' : ms < 100 ? 'text-yellow-500' : 'text-red-500';
};

/** Minecraft `§` colour codes -> hex, for motd previews. */
export function stripMinecraftColors(value: string): string {
  return value.replace(/§[0-9a-fk-or]/gi, '');
}
