import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { ApiKey, CreateKeyReq } from '@/types';

export function useApiKeys(userId?: string) {
  const path = userId ? `/users/${encodeURIComponent(userId)}/keys` : '/me/keys';
  return useQuery({
    queryKey: ['keys', userId],
    queryFn: () => api<ApiKey[]>(path),
  });
}

export function useCreateApiKey(userId?: string) {
  const qc = useQueryClient();
  const path = userId ? `/users/${encodeURIComponent(userId)}/keys` : '/me/keys';
  return useMutation({
    mutationFn: (data: CreateKeyReq) =>
      api<{ key: string; user_id: string; name: string; enabled: boolean }>(path, {
        method: 'POST',
        body: data,
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['keys'] }),
  });
}

export function useDeleteApiKey(userId?: string) {
  const qc = useQueryClient();
  const basePath = userId ? `/users/${encodeURIComponent(userId)}/keys` : '/me/keys';
  return useMutation({
    mutationFn: (keyVal: string) =>
      api<void>(`${basePath}/${encodeURIComponent(keyVal)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['keys'] }),
  });
}
