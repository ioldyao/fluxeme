import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './client';

export interface SchedulerModelSummary {
  id: string;
  name: string;
  published: boolean;
  binding_count: number;
}

export interface SchedulerEndpointPolicy {
  endpoint_id: number;
  weight: number;
  timeout_secs: number | null;
  max_tokens: number | null;
}

/** A channel is a group of endpoints; it carries no scheduling parameters of
 *  its own. The scheduler selects endpoints directly. */
export interface SchedulerBindingPolicy {
  channel_id: string;
  endpoints: SchedulerEndpointPolicy[];
}

export interface SchedulerModelPolicy {
  model_id: string;
  model_name: string;
  bindings: SchedulerBindingPolicy[];
}

export interface SchedulerEndpointTopology {
  endpoint_id: number;
  url: string;
  weight: number;
  timeout_secs: number | null;
  max_tokens: number | null;
  routing_available: boolean;
  routing_state: string;
  routing_reason: string;
  circuit_state: string;
  observed_endpoint_share_24h: number | null;
}

export interface SchedulerBindingTopology {
  channel_id: string;
  channel_name: string;
  provider: string;
  upstream_model: string | null;
  channel_enabled: boolean;
  endpoint_count: number;
  configured_total_weight: number;
  eligible_total_weight: number;
  configured_share: number | null;
  eligible_share: number | null;
  routing_state: string;
  routing_reason: string;
  request_count_24h: number;
  observed_model_share_24h: number | null;
  endpoints: SchedulerEndpointTopology[];
}

export interface SchedulerTopologyResponse {
  model: string;
  configured_total_weight: number;
  eligible_total_weight: number;
  bindings: SchedulerBindingTopology[];
}

export function useSchedulerModels() {
  return useQuery({
    queryKey: ['scheduler', 'models'],
    queryFn: () => api<SchedulerModelSummary[]>('/scheduler/models'),
  });
}

export function useSchedulerModelPolicy(modelId: string) {
  return useQuery({
    queryKey: ['scheduler', 'policy', modelId],
    queryFn: () => api<SchedulerModelPolicy>(`/scheduler/models/${encodeURIComponent(modelId)}/policy`),
    enabled: !!modelId,
  });
}

export function useSchedulerTopology(modelId: string) {
  return useQuery({
    queryKey: ['scheduler', 'topology', modelId],
    queryFn: () => api<SchedulerTopologyResponse>(`/scheduler/models/${encodeURIComponent(modelId)}/topology`),
    enabled: !!modelId,
    refetchInterval: 10_000,
  });
}

export function useSaveSchedulerModelPolicy(modelId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (policy: SchedulerModelPolicy) =>
      api<{ ok: boolean }>(`/scheduler/models/${encodeURIComponent(modelId)}/policy`, {
        method: 'PUT',
        body: policy,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['scheduler', 'policy', modelId] });
      qc.invalidateQueries({ queryKey: ['scheduler', 'topology', modelId] });
    },
  });
}
