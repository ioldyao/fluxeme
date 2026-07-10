import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { UsageRecord, DailyAggregate } from '@/types';

interface UsageParams {
  limit?: number;
  user_id?: string;
}

export function useUsage(params: UsageParams = {}) {
  const searchParams = new URLSearchParams();
  if (params.limit) searchParams.set('limit', String(params.limit));
  if (params.user_id) searchParams.set('user_id', params.user_id);
  const qs = searchParams.toString();

  // Serialize to prevent object-reference instability causing infinite refetch
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['usage', stableKey],
    queryFn: () => api<UsageRecord[]>(`/usage${qs ? `?${qs}` : ''}`),
  });
}

export function useUsageDetail(requestId: string | null) {
  return useQuery({
    queryKey: ['usage', requestId],
    queryFn: () => api<UsageRecord>(`/usage/${requestId}`),
    enabled: !!requestId,
  });
}

export function useUsageAggregate(days: number = 14) {
  return useQuery({
    queryKey: ['usage', 'aggregate', days],
    queryFn: () => api<DailyAggregate[]>(`/usage/aggregate?days=${days}`),
  });
}
