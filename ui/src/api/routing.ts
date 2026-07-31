import { useQuery } from '@tanstack/react-query';
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
  endpoints: RoutingHistoryEndpoint[];
}

export interface RoutingHistoryEndpoint {
  endpoint_id: number | null;
  url: string;
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

export async function fetchRoutingFlowSnapshot(): Promise<Record<string, number>> {
  const raw = await api<[string, string, number | null, number][]>("/routing/snapshot");
  const counts: Record<string, number> = {};
  for (const [model, chId, epId, cnt] of raw) {
    const keyFor = (...p: (string | number)[]) => p.join(">");
    counts[keyFor(model)] = (counts[keyFor(model)] || 0) + cnt;
    counts[keyFor(model, chId)] = (counts[keyFor(model, chId)] || 0) + cnt;
    if (epId != null) counts[keyFor(model, chId, `id:${epId}`)] = (counts[keyFor(model, chId, `id:${epId}`)] || 0) + cnt;
  }
  return counts;
}

function routingWindow(days: number) {
  const now = new Date();
  const start = new Date(now.getTime() - days * 86400000);
  return {
    start: start.toISOString().replace('T', ' ').slice(0, 19),
    end: now.toISOString().replace('T', ' ').slice(0, 19),
  };
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

export function useRoutingHistory(
  days: number,
  opts?: { model?: string; enabled?: boolean },
) {
  return useQuery({
    queryKey: ['routing', 'history', days, opts?.model ?? 'all'],
    queryFn: () => {
      const { start, end } = routingWindow(days);
      return fetchRoutingHistory(start, end, opts?.model);
    },
    enabled: opts?.enabled !== false,
    refetchInterval: 60_000,
  });
}

// ── Model monitoring (model × channel 24h health snapshot) ─────────

export interface RoutingHealthEndpoint {
  endpoint_id: number | null;
  enabled: boolean;
  available: boolean;
}

export interface RoutingHealthChannel {
  channel_id: string;
  channel_name: string;
  priority: number;
  provider?: string;
  requests: number;
  success_rate: number;
  avg_latency_ms: number;
  p95_latency_ms: number;
  circuit_ok: boolean;
  circuit_enabled: boolean;
  endpoints: RoutingHealthEndpoint[];
}

export interface RoutingHealthModel {
  id: string;
  name: string;
  model_pattern: string;
  category?: string;
  total_requests: number;
  channels: RoutingHealthChannel[];
}

export interface RoutingHealthResponse {
  models: RoutingHealthModel[];
  summary: {
    total_requests_24h: number;
    overall_success_rate: number;
    active_channels: number;
    broken_channels: number;
  };
}

/** Per-model × channel 24h health snapshot from GET /api/health/routing. */
export function useRoutingHealth(opts?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['routing', 'health'],
    queryFn: () => api<RoutingHealthResponse>('/health/routing'),
    refetchInterval: 30_000,
    enabled: opts?.enabled !== false,
  });
}
