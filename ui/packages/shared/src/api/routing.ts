import { useQuery } from '@tanstack/react-query';
import { api } from './client';

export interface RoutingRecentPath {
  timestamp: string;
  model: string;
  channel_id: string;
  endpoint_id: number | null;
  endpoint_url?: string | null;
  latency_ms: number;
  success: boolean;
}

export interface RoutingHistoryChannelSeries {
  channel_name: string;
  requests: number[];
  successes: number[];
  success_rate_percent: Array<number | null>;
}

export interface RoutingHistoryEndpoint {
  endpoint_id: number | null;
  url: string | null;
  url_status: 'stable' | 'varied' | 'missing';
  url_variant_count: number;
  requests: number;
  successes: number;
  success_rate_percent: number | null;
  avg_latency_ms: number | null;
  p95_latency_ms: number | null;
}

export interface RoutingHistorySummary {
  channel_id: string;
  requests: number;
  successes: number;
  success_rate_percent: number | null;
  avg_latency_ms: number | null;
  p95_latency_ms: number | null;
  endpoints: RoutingHistoryEndpoint[];
}

export interface RoutingHistoryResponse {
  schema_version: 2;
  timezone: 'UTC';
  bucket_unit: 'hour' | 'day';
  buckets: string[];
  series: Record<string, RoutingHistoryChannelSeries>;
  totals: {
    requests: number;
    successes: number;
    success_rate_percent: number | null;
    avg_latency_ms: number | null;
    p95_latency_ms: number | null;
    unattributed_requests: number;
  };
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

export async function fetchRecentRoutingPaths(): Promise<RoutingRecentPath[]> {
  const res = await api<{ paths: RoutingRecentPath[] }>("/health/recent-paths");
  return res.paths;
}

function routingWindow(days: number) {
  const now = new Date();
  const start = new Date(now.getTime() - days * 86400000);
  return {
    start: start.toISOString(),
    end: now.toISOString(),
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
  enabled: boolean;
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

/** Per-model × channel 24h health snapshot from GET /api/health/routing.
 *  Polled every 10s — endpoint availability reflects the live circuit
 *  breaker, which is now fed by real traffic and the 60s auto-probe task. */
export function useRoutingHealth(opts?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['routing', 'health'],
    queryFn: () => api<RoutingHealthResponse>('/health/routing'),
    refetchInterval: 10_000,
    enabled: opts?.enabled !== false,
  });
}

export interface EndpointLiveHealth {
  endpoint_id: number;
  enabled: boolean;
  healthy_bindings: number;
  total_bindings: number;
  long_unavailable: boolean;
  available: boolean;
}

export interface EndpointsLiveHealthResponse {
  endpoints: EndpointLiveHealth[];
}

/** Realtime per-endpoint circuit-breaker health aggregated over all published
 *  model bindings — reflects binding_pool state, not 24h ClickHouse aggregates.
 *  Polled every 10s. */
export function useEndpointsLiveHealth(opts?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['routing', 'endpoints-live'],
    queryFn: () => api<EndpointsLiveHealthResponse>('/health/endpoints-live'),
    refetchInterval: 10_000,
    enabled: opts?.enabled !== false,
  });
}
