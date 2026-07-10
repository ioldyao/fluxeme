import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { Model, Pricing } from '@/types';

export function useModels() {
  return useQuery({
    queryKey: ['models'],
    queryFn: () => api<Model[]>('/models'),
  });
}

export function usePublicModels() {
  return useQuery({
    queryKey: ['models', 'public'],
    queryFn: () => api<Model[]>('/models/public'),
  });
}

export function usePublishModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<{ id: string; published: boolean }>(`/models/${encodeURIComponent(id)}/publish`, { method: 'POST' }),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['models'] }); qc.invalidateQueries({ queryKey: ['models', 'public'] }); },
  });
}

export function useSubscriptions() {
  return useQuery({
    queryKey: ['me', 'subscriptions'],
    queryFn: () => api<Model[]>('/me/subscriptions'),
  });
}

export function useSubscribeModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (modelId: string) =>
      api<{ subscribed: string }>(`/me/subscriptions/${encodeURIComponent(modelId)}`, { method: 'POST' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['me', 'subscriptions'] }),
  });
}

export function useUnsubscribeModel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (modelId: string) =>
      api<{ unsubscribed: string }>(`/me/subscriptions/${encodeURIComponent(modelId)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['me', 'subscriptions'] }),
  });
}

export interface ModelTestResult {
  success: boolean;
  error?: string;
  latency_ms?: number;
}

export function useTestModelConnection() {
  return useMutation({
    mutationFn: (modelId: string) =>
      api<ModelTestResult>('/me/test-connection', { method: 'POST', body: { model_id: modelId } }),
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

export function useUpdateModelPricing() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, pricing }: { id: string; pricing: Pricing }) =>
      api<{ ok: boolean }>(`/models/${encodeURIComponent(id)}/pricing`, {
        method: 'PATCH',
        body: pricing,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['models'] });
    },
  });
}
