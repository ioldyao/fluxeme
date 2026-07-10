import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { UserRole, LoginResponse } from '@/types';

interface AuthState {
  token: string | null;
  role: UserRole | null;
  userId: string | null;
  userName: string | null;
  setSession: (res: LoginResponse) => void;
  clear: () => void;
}

export const useAuth = create<AuthState>()(
  persist(
    (set) => ({
      token: null,
      role: null,
      userId: null,
      userName: null,
      setSession: (res) =>
        set({
          token: res.token,
          role: res.role,
          userId: res.user_id,
          userName: res.user_name,
        }),
      clear: () =>
        set({
          token: null,
          role: null,
          userId: null,
          userName: null,
        }),
    }),
    {
      name: 'auth',
      storage: createJSONStorage(() => sessionStorage),
    },
  ),
);
