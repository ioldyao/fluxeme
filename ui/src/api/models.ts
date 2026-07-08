import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { Model } from '@/types';

export function useModels() {
  return useQuery({
    queryKey: ['models'],
    queryFn: () => api<Model[]>('/models'),
  });
}

export function useCreateModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<Model>) =>
      api<Model>('/models', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['models'] }),
  });
}

export function useUpdateModel(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<Model>) =>
      api<Model>(`/models/${encodeURIComponent(id)}`, { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['models'] }),
  });
}

export function useDeleteModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/models/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['models'] }),
  });
}
