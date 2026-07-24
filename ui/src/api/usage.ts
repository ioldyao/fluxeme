import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { UsageRecord, DailyAggregate, ModelActivity } from '@/types';

interface UsageParams {
  limit?: number;
  offset?: number;
  user_id?: string;
  model?: string;
  api_key?: string;
  api_format?: string;
  start_date?: string;
  end_date?: string;
}

interface UsageResponse {
  records: UsageRecord[];
  total: number;
}

export function useUsage(params: UsageParams = {}) {
  const searchParams = new URLSearchParams();
  if (params.limit) searchParams.set('limit', String(params.limit));
  if (params.offset) searchParams.set('offset', String(params.offset));
  if (params.user_id) searchParams.set('user_id', params.user_id);
  if (params.model) searchParams.set('model', params.model);
  if (params.api_key) searchParams.set('api_key', params.api_key);
  if (params.api_format) searchParams.set('api_format', params.api_format);
  if (params.start_date) searchParams.set('start_date', params.start_date);
  if (params.end_date) searchParams.set('end_date', params.end_date);
  const qs = searchParams.toString();

  // Serialize to prevent object-reference instability causing infinite refetch
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['usage', stableKey],
    queryFn: () => api<UsageResponse>(`/usage${qs ? `?${qs}` : ''}`),
    refetchInterval: 60_000,
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
    refetchInterval: 60_000,
  });
}

export interface FunnelStats {
  total: number;
  success_count: number;
  auth_fail_count: number;
  rate_limit_count: number;
  bad_request_count: number;
  upstream_error_count: number;
  timeout_count: number;
  other_error_count: number;
  p50_latency: number;
  p95_latency: number;
  p99_latency: number;
  avg_latency: number;
}

export function useUsageFunnel(days: number) {
  return useQuery({
    queryKey: ['usage', 'funnel', days],
    queryFn: () => api<FunnelStats>(`/usage/funnel?days=${days}`),
    refetchInterval: 60_000,
  });
}

export function useModelActivity(days: number = 7) {
  return useQuery({
    queryKey: ['usage', 'model-activity', days],
    queryFn: () => api<ModelActivity[]>(`/usage/model-activity?days=${days}`),
    refetchInterval: 60_000,
  });
}
