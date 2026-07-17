import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { ContentFilterRule } from '@/types';

export function useFilterRules() {
  return useQuery({
    queryKey: ['filter-rules'],
    queryFn: () => api<ContentFilterRule[]>('/moderation/rules'),
  });
}

export function useCreateFilterRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<ContentFilterRule>) =>
      api<{ id: string }>('/moderation/rules', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['filter-rules'] }),
  });
}

export function useUpdateFilterRule(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<ContentFilterRule>) =>
      api<void>(`/moderation/rules/${encodeURIComponent(id)}`, { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['filter-rules'] }),
  });
}

export function useDeleteFilterRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/moderation/rules/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['filter-rules'] }),
  });
}
