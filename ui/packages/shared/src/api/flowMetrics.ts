import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { FlowMetricsResponse } from '../types';

export interface FlowMetricsParams {
  start?: string;
  end?: string;
  model?: string;
}

export interface FlowMetricsQueryOptions {
  enabled?: boolean;
  refetchInterval?: number | false;
}

function buildFlowMetricsSearchParams(params: FlowMetricsParams = {}) {
  const searchParams = new URLSearchParams();
  if (params.start) searchParams.set('start', params.start);
  if (params.end) searchParams.set('end', params.end);
  if (params.model) searchParams.set('model', params.model);
  return searchParams;
}

export function useFlowMetrics(
  params: FlowMetricsParams = {},
  options: FlowMetricsQueryOptions = {},
) {
  const qs = buildFlowMetricsSearchParams(params).toString();
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['flow-metrics', stableKey],
    queryFn: () => api<FlowMetricsResponse>(`/health/flow-metrics${qs ? `?${qs}` : ''}`),
    refetchInterval: options.refetchInterval ?? 30_000,
    enabled: options.enabled !== false,
  });
}
