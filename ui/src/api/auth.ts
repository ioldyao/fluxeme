import { useMutation } from '@tanstack/react-query';
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
