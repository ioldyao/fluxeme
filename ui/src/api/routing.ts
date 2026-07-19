import { api } from './client';

export interface RoutingHistoryChannelSeries {
  channel_name: string;
  volume: number[];
  success_rate: number[];
}

export interface RoutingHistorySummary {
  channel_id: string;
  requests: number;
  success_rate: number;
  avg_latency: number;
  p95_latency: number;
}

export interface RoutingHistoryResponse {
  buckets: string[];
  series: Record<string, RoutingHistoryChannelSeries>;
  summary: RoutingHistorySummary[];
}

export async function fetchRoutingHistory(
  start: string,
  end: string,
  model?: string,
): Promise<RoutingHistoryResponse> {
  const params = new URLSearchParams({ start, end });
  if (model && model !== 'all') params.set('model', model);
  return api<RoutingHistoryResponse>(`/routing/history?${params.toString()}`);
}
