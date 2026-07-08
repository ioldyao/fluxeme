import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { RoutingRule } from '@/types';

export function useRules() {
  return useQuery({
    queryKey: ['rules'],
    queryFn: () => api<RoutingRule[]>('/rules'),
  });
}

export function useCreateRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<RoutingRule>) =>
      api<RoutingRule>('/rules', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }),
  });
}

export function useUpdateRule(name: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<RoutingRule>) =>
      api<RoutingRule>(`/rules/${encodeURIComponent(name)}`, { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }),
  });
}

export function useDeleteRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) =>
      api<void>(`/rules/${encodeURIComponent(name)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }),
  });
}
