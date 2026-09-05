import { useAuth } from '../store/auth';

/** Parse a ClickHouse timestamp string into a Date.
 *
 *  The CH server runs UTC, so `toString(DateTime)` returns a wall-clock
 *  string without a timezone designator ("2026-08-04 07:15:30"). Plain
 *  `new Date(...)` would treat that as browser-local time, so normalize it
 *  to an explicit UTC instant. `formatDateTime` already emits an ISO string
 *  with a Z suffix, which parses correctly as UTC on its own.
 *
 *  Some endpoints (gateway request lifecycle) return the raw epoch **seconds**
 *  (`toUInt32(timestamp)`). Treat a numeric input as epoch seconds so the
 *  display does not fall back to 1970.
 */
export function parseTimestamp(ts: string | number): Date {
  if (typeof ts === 'number') {
    return new Date(ts * 1000);
  }
  const normalized = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}/.test(ts)
    ? `${ts.replace(' ', 'T')}Z`
    : ts;
  const date = new Date(normalized);
  return Number.isNaN(date.getTime()) ? new Date(ts) : date;
}

/** The user's configured display timezone, falling back to UTC. */
export function getDisplayTimezone(): string {
  return useAuth.getState().timezone || 'UTC';
}

/** Format a Date in the user's configured timezone, falling back to the
 *  browser timezone if the configured one is invalid. */
function formatInTimezone(date: Date, opts: Intl.DateTimeFormatOptions): string {
  const timeZone = getDisplayTimezone();
  try {
    return new Intl.DateTimeFormat(undefined, { ...opts, timeZone }).format(date);
  } catch {
    return new Intl.DateTimeFormat(undefined, opts).format(date);
  }
}

/** Full datetime, e.g. "8/4/2026, 3:15:30 PM", in the user's timezone. */
export function formatTimestamp(ts: string | number): string {
  return formatInTimezone(parseTimestamp(ts), {
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit',
  });
}

/** Time-only, e.g. "3:15:30 PM", in the user's timezone. */
export function formatTime(date: Date): string {
  return formatInTimezone(date, {
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit',
  });
}

/** Time-only for a timestamp string, e.g. "3:15:30 PM". */
export function formatTimestampTime(ts: string): string {
  return formatTime(parseTimestamp(ts));
}
