import { create } from 'zustand';
import { persist } from 'zustand/middleware';

type Theme = 'dark' | 'light';

interface ThemeState {
  theme: Theme;
  toggle: () => void;
  setTheme: (theme: Theme) => void;
}

function applyTheme(theme: Theme) {
  if (typeof document !== 'undefined') {
    document.documentElement.classList.toggle('dark', theme === 'dark');
  }
}

const initialTheme: Theme =
  typeof window !== 'undefined'
    ? (localStorage.getItem('theme') as Theme) || 'dark'
    : 'dark';
applyTheme(initialTheme);

export const useTheme = create<ThemeState>()(
  persist(
    (set) => ({
      theme: initialTheme,
      toggle: () =>
        set((s) => {
          const next = s.theme === 'dark' ? 'light' : 'dark';
          applyTheme(next);
          return { theme: next };
        }),
      setTheme: (theme) => {
        applyTheme(theme);
        set({ theme });
      },
    }),
    { name: 'theme' },
  ),
);
