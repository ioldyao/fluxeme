import { useMutation, useQuery } from '@tanstack/react-query';
import { api } from './client';
import { useAuth } from '@/store/auth';
import type { LoginResponse } from '@/types';

interface LoginInput {
  username: string;
  password: string;
}

export function useLogin() {
  const setSession = useAuth((s) => s.setSession);
  return useMutation({
    mutationFn: (data: LoginInput) =>
      api<LoginResponse>('/login', {
        method: 'POST',
        body: data,
      }),
    onSuccess: (res) => {
      setSession(res);
    },
  });
}

export function useUpdateTimezone() {
  const setTimezone = useAuth((s) => s.setTimezone);
  return useMutation({
    mutationFn: (timezone: string) =>
      api<{ timezone: string }>('/me/timezone', {
        method: 'PUT',
        body: { timezone },
      }),
    onSuccess: (res) => {
      setTimezone(res.timezone);
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
