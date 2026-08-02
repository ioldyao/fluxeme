import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import { useAuth } from '../store/auth';
import type { CurrentSessionResponse, LoginResponse } from '../types';

interface LoginInput {
  username: string;
  password: string;
}

interface SessionResponse {
  authenticated: boolean;
  user?: {
    id: string;
    name: string;
    role: string;
  };
  permissions?: string[];
  portals?: {
    user: boolean;
    admin: boolean;
  };
}

export function useLogin() {
  const setSession = useAuth((s) => s.setSession);
  return useMutation({
    mutationFn: (data: LoginInput) =>
      api<LoginResponse>('/login', {
        method: 'POST',
        body: data,
        skipAuthErrorHandling: true,
      }),
    onSuccess: (res) => {
      setSession(res);
    },
  });
}

/** Session probe — always returns 200. Use instead of /api/me for initial load. */
export function useSessionQuery() {
  return useQuery({
    queryKey: ['auth', 'session'],
    queryFn: () => api<SessionResponse>('/auth/session', { skipAuthErrorHandling: true }),
    retry: false,
    staleTime: 30_000,
  });
}

/** Current session detail — only call when authenticated. */
export function useCurrentSession(enabled = true) {
  return useQuery({
    queryKey: ['auth', 'current-session'],
    queryFn: () => api<CurrentSessionResponse>('/me', { skipAuthErrorHandling: true }),
    enabled,
    retry: false,
  });
}

export function useLogout() {
  return useMutation({
    mutationFn: () => api<{ ok: boolean }>('/logout', { method: 'POST', skipAuthErrorHandling: true }),
  });
}

export function useUpdateTimezone() {
  const qc = useQueryClient();
  const setTimezone = useAuth((s) => s.setTimezone);
  return useMutation({
    mutationFn: (timezone: string) =>
      api<{ timezone: string }>('/me/timezone', {
        method: 'PUT',
        body: { timezone },
      }),
    onSuccess: (res) => {
      setTimezone(res.timezone);
      void qc.invalidateQueries({ queryKey: ['dashboard', 'self'] });
      void qc.invalidateQueries({ queryKey: ['usage', 'aggregate', 'self'] });
      void qc.invalidateQueries({ queryKey: ['usage', 'funnel', 'self'] });
      void qc.invalidateQueries({ queryKey: ['usage', 'model-activity', 'self'] });
    },
  });
}

export function useSetupStatus() {
  return useQuery({
    queryKey: ['setup-status'],
    queryFn: () => api<{ setup_required: boolean }>('/setup/status'),
    staleTime: Infinity,
  });
}

export function useSetupRegister() {
  return useMutation({
    mutationFn: (data: { username: string; password: string }) =>
      api<{ ok: boolean }>('/setup/register', {
        method: 'POST',
        body: data,
      }),
  });
}
