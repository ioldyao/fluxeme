import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { queryClient } from '@fluxeme/shared/query';
import { useCurrency, type CurrencyCode } from '@/store/currency';
import type { CurrentSessionResponse, LoginResponse, UserRole } from '@fluxeme/shared/types';

function toCurrencyCode(currency: string): CurrencyCode {
  return currency === 'cny' ? 'cny' : 'usd';
}

interface AuthState {
  role: UserRole | null;
  userId: string | null;
  userName: string | null;
  timezone: string;
  currency: string;
  permissions: string[];
  isAuthenticated: boolean;
  isSessionResolved: boolean;
  setSession: (res: LoginResponse) => void;
  setCurrentSession: (res: CurrentSessionResponse) => void;
  setPermissions: (perms: string[]) => void;
  setTimezone: (tz: string) => void;
  setCurrency: (c: string) => void;
  clear: () => void;
}

export const useAuth = create<AuthState>()(
  persist(
    (set) => ({
      role: null,
      userId: null,
      userName: null,
      timezone: 'UTC',
      currency: 'usd',
      permissions: [],
      isAuthenticated: false,
      isSessionResolved: false,
      setSession: (res) => {
        const nextCurrency = toCurrencyCode(res.currency || 'usd');
        queryClient.clear();
        useCurrency.getState().setCurrency(nextCurrency);
        set({
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone || 'UTC',
          currency: nextCurrency,
          isAuthenticated: true,
          isSessionResolved: true,
        });
      },
      setCurrentSession: (res) => {
        const nextCurrency = toCurrencyCode(res.currency);
        useCurrency.getState().setCurrency(nextCurrency);
        set({
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone,
          currency: nextCurrency,
          isAuthenticated: true,
          isSessionResolved: true,
        });
      },
      setPermissions: (permissions) => set({ permissions }),
      setTimezone: (timezone) => set({ timezone }),
      setCurrency: (currency) => {
        const nextCurrency = toCurrencyCode(currency);
        useCurrency.getState().setCurrency(nextCurrency);
        set({ currency: nextCurrency });
      },
      clear: () => {
        queryClient.clear();
        useCurrency.getState().setCurrency('usd');
        set({
          role: null,
          userId: null,
          userName: null,
          timezone: 'UTC',
          currency: 'usd',
          permissions: [],
          isAuthenticated: false,
          isSessionResolved: true,
        });
      },
    }),
    {
      name: 'auth',
      storage: createJSONStorage(() => localStorage),
      partialize: (state) => ({
        role: state.role,
        userId: state.userId,
        userName: state.userName,
        timezone: state.timezone,
        currency: state.currency,
        permissions: state.permissions,
      }),
    },
  ),
);
