import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type ThemeMode = 'dark' | 'light' | 'system';

interface ThemeState {
  mode: ThemeMode;
  resolved: 'dark' | 'light';
  setMode: (mode: ThemeMode) => void;
  toggle: () => void;
}

function getSystemTheme(): 'dark' | 'light' {
  if (typeof window === 'undefined') return 'dark';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyTheme(mode: ThemeMode) {
  if (typeof document === 'undefined') return;
  const theme = mode === 'system' ? getSystemTheme() : mode;
  document.documentElement.classList.toggle('dark', theme === 'dark');
}

function getStored(): ThemeMode {
  if (typeof window === 'undefined') return 'system';
  try {
    const stored = localStorage.getItem('theme-mode');
    if (stored === 'dark' || stored === 'light' || stored === 'system') return stored;
  } catch {}
  return 'system';
}

const initial = getStored();
applyTheme(initial);

let mediaQuery: MediaQueryList | null = null;
if (typeof window !== 'undefined') {
  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
}

export const useTheme = create<ThemeState>()(
  persist(
    (set) => {
      // Listen for OS theme changes when mode is 'system'
      const handler = () => {
        const mode = getStored();
        if (mode === 'system') {
          applyTheme('system');
          set({ resolved: getSystemTheme() });
        }
      };
      if (mediaQuery) mediaQuery.addEventListener('change', handler);

      return {
        mode: initial,
        resolved: initial === 'system' ? getSystemTheme() : initial,
        setMode: (mode) => {
          applyTheme(mode);
          set({ mode, resolved: mode === 'system' ? getSystemTheme() : mode });
        },
        toggle: () =>
          set((s) => {
            const next = s.resolved === 'dark' ? 'light' : 'dark';
            applyTheme(next);
            return { mode: next, resolved: next };
          }),
      };
    },
    { name: 'theme-mode' },
  ),
);
