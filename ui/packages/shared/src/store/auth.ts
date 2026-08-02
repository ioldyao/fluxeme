import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { queryClient } from '../lib/query';
import type { CurrentSessionResponse, LoginResponse, UserRole } from '../types';

interface AuthState {
  role: UserRole | null;
  userId: string | null;
  userName: string | null;
  timezone: string;
  permissions: string[];
  isAuthenticated: boolean;
  isSessionResolved: boolean;
  setSession: (res: LoginResponse) => void;
  setCurrentSession: (res: CurrentSessionResponse) => void;
  setPermissions: (perms: string[]) => void;
  setTimezone: (tz: string) => void;
  clear: () => void;
}

export const useAuth = create<AuthState>()(
  persist(
    (set) => ({
      role: null,
      userId: null,
      userName: null,
      timezone: 'UTC',
      permissions: [],
      isAuthenticated: false,
      isSessionResolved: false,
      setSession: (res) => {
        queryClient.clear();
        set({
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone || 'UTC',
          isAuthenticated: true,
          isSessionResolved: true,
        });
      },
      setCurrentSession: (res) => {
        set({
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
          timezone: res.timezone,
          isAuthenticated: true,
          isSessionResolved: true,
        });
      },
      setPermissions: (perms) => set({ permissions: perms }),
      setTimezone: (timezone) => set({ timezone }),
      clear: () => {
        queryClient.clear();
        set({
          role: null,
          userId: null,
          userName: null,
          timezone: 'UTC',
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
        permissions: state.permissions,
      }),
    },
  ),
);
