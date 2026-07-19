import { useQuery } from '@tanstack/react-query';
import { api } from './client';

export interface EndpointHealth {
  endpoint_id: number;
  enabled: boolean;
  available: boolean;
}

export interface ChannelRouteHealth {
  channel_id: string;
  channel_name: string;
  priority: number;
  provider: string;
  requests: number;
  success_rate: number;
  avg_latency_ms: number;
  p95_latency_ms: number;
  circuit_ok: boolean;
  circuit_enabled: boolean;
  endpoints: EndpointHealth[];
}

export interface ModelRouteHealth {
  id: string;
  name: string;
  model_pattern: string;
  category: string;
  total_requests: number;
  channels: ChannelRouteHealth[];
}

export interface RoutingHealthSummary {
  total_requests_24h: number;
  overall_success_rate: number;
  active_channels: number;
  broken_channels: number;
}

export interface RoutingHealthData {
  models: ModelRouteHealth[];
  summary: RoutingHealthSummary;
}

export interface RecentPath {
  timestamp: string;
  model: string;
  channel_id: string;
  latency_ms: number;
  success: boolean;
}

export function useRecentPaths() {
  return useQuery({
    queryKey: ['health', 'recent-paths'],
    queryFn: () => api<{ paths: RecentPath[] }>('/health/recent-paths'),
    refetchInterval: 5_000,
  });
}

export function useRoutingHealth() {
  return useQuery({
    queryKey: ['health', 'routing'],
    queryFn: () => api<RoutingHealthData>('/health/routing'),
    refetchInterval: 30_000,
  });
}
