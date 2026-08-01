import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { RoutingRule } from '@fluxeme/shared/types';

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

export function useUpdateRule(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<RoutingRule>) =>
      api<RoutingRule>(`/rules/${encodeURIComponent(id)}`, { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }),
  });
}

export function useDeleteRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/rules/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }),
  });
}

// ── User-level rules (self-service) ──────────────────────────

export function useMyRules() {
  return useQuery({
    queryKey: ['me', 'rules'],
    queryFn: () => api<RoutingRule[]>('/me/rules'),
  });
}

export function useCreateMyRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: { source_model: string; target_model: string }) =>
      api<RoutingRule>('/me/rules', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['me', 'rules'] }),
  });
}

export function useDeleteMyRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/me/rules/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['me', 'rules'] }),
  });
}
