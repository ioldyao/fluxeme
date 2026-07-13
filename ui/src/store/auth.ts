import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { UserRole, LoginResponse } from '@/types';
import { useCurrency, type CurrencyCode } from './currency';

interface AuthState {
  token: string | null;
  role: UserRole | null;
  userId: string | null;
  userName: string | null;
  timezone: string;
  displayCurrency: string;
  setSession: (res: LoginResponse) => void;
  setTimezone: (tz: string) => void;
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
      displayCurrency: 'usd',
      setSession: (res) => {
        const currency = (res.display_currency || 'usd') as CurrencyCode;
        set({
          token: res.token,
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone || 'UTC',
          displayCurrency: currency,
        });
        useCurrency.getState().setCurrency(currency);
      },
      setTimezone: (timezone) => set({ timezone }),
      clear: () =>
        set({
          token: null,
          role: null,
          userId: null,
          userName: null,
          timezone: 'UTC',
          displayCurrency: 'usd',
        }),
    }),
    {
      name: 'auth',
      storage: createJSONStorage(() => localStorage),
    },
  ),
);
