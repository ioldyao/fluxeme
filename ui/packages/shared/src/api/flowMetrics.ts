import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { FlowMetricsResponse } from '../types';

export interface FlowMetricsParams {
  start?: string;
  end?: string;
  model?: string;
}

function buildFlowMetricsSearchParams(params: FlowMetricsParams = {}) {
  const searchParams = new URLSearchParams();
  if (params.start) searchParams.set('start', params.start);
  if (params.end) searchParams.set('end', params.end);
  if (params.model) searchParams.set('model', params.model);
  return searchParams;
}

export function useFlowMetrics(params: FlowMetricsParams = {}) {
  const qs = buildFlowMetricsSearchParams(params).toString();
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['flow-metrics', stableKey],
    queryFn: () => api<FlowMetricsResponse>(`/health/flow-metrics${qs ? `?${qs}` : ''}`),
    refetchInterval: 30_000,
  });
}
