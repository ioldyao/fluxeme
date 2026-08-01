import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { Model, Pricing, ModelHealthCheckResult } from '@fluxeme/shared/types';

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

export function useModelHealthCheck() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ modelId, channelIds, stream }: { modelId: string; channelIds: string[]; stream: boolean }) =>
      api<ModelHealthCheckResult>(`/models/${encodeURIComponent(modelId)}/health-check`, {
        method: 'POST',
        body: { channel_ids: channelIds, stream },
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['probe-results'] }),
  });
}
