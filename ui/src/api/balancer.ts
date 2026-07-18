import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';

export interface EndpointHealthItem {
  endpoint_id: number;
  url: string;
  enabled: boolean;
  available: boolean;
}

export interface ChannelHealthResponse {
  channel_id: string;
  endpoints: EndpointHealthItem[];
  probe_success?: boolean | null;
  probe_latency_ms?: number | null;
}

export function useChannelHealth(channelId: string) {
  return useQuery({
    queryKey: ['channel-health', channelId],
    queryFn: () => api<ChannelHealthResponse>(`/channels/${encodeURIComponent(channelId)}/health`),
    enabled: !!channelId,
    refetchInterval: 10_000,
  });
}

export function useToggleEndpoint() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: number; enabled: boolean }) =>
      api(`/endpoints/${id}`, { method: 'PATCH', body: { enabled } }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['channels'] });
      qc.invalidateQueries({ queryKey: ['channel-health'] });
    },
  });
}
