import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface TimezoneState {
  timezone: string;
  setTimezone: (tz: string) => void;
}

function detectBrowserTz(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

export const useTimezone = create<TimezoneState>()(
  persist(
    (set) => ({
      timezone: detectBrowserTz(),
      setTimezone: (timezone) => set({ timezone }),
    }),
    { name: 'timezone' },
  ),
);

export const COMMON_TIMEZONES: string[] = [
  'UTC',
  'Asia/Shanghai',
  'Asia/Hong_Kong',
  'Asia/Tokyo',
  'Asia/Seoul',
  'Asia/Singapore',
  'Asia/Taipei',
  'Asia/Bangkok',
  'Asia/Kolkata',
  'Asia/Dubai',
  'Europe/London',
  'Europe/Paris',
  'Europe/Berlin',
  'Europe/Moscow',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'America/Sao_Paulo',
  'Australia/Sydney',
  'Pacific/Auckland',
];
