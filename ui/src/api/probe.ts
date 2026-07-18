import { useQuery } from '@tanstack/react-query';
import { api } from './client';

export interface ProbeResult {
  id: string;
  channel_id: string;
  model_id: string;
  success: boolean;
  latency_ms: number;
  error?: string | null;
  probed_at: string;
}

export function useProbeResults() {
  return useQuery({
    queryKey: ['probe-results'],
    queryFn: () => api<ProbeResult[]>('/probe-results'),
    refetchInterval: 30_000,
  });
}
