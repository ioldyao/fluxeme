import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { UserRole, LoginResponse } from '@/types';

interface AuthState {
  token: string | null;
  role: UserRole | null;
  userId: string | null;
  userName: string | null;
  timezone: string;
  currency: string;
  setSession: (res: LoginResponse) => void;
  setTimezone: (tz: string) => void;
  setCurrency: (c: string) => void;
  clear: () => void;
}

export const useAuth = create<AuthState>()(
  persist(
    (set) => ({
      token: null,
      role: null,
      userId: null,
      userName: null,
      timezone: 'UTC',
      currency: 'usd',
      setSession: (res) =>
        set({
          token: res.token,
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone || 'UTC',
          currency: res.currency || 'usd',
        }),
      setTimezone: (timezone) => set({ timezone }),
      setCurrency: (currency) => set({ currency }),
      clear: () =>
        set({
          token: null,
          role: null,
          userId: null,
          userName: null,
          timezone: 'UTC',
          currency: 'usd',
        }),
    }),
    {
      name: 'auth',
      storage: createJSONStorage(() => localStorage),
    },
  ),
);
