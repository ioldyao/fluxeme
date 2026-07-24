import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { ProbeResult } from '@/types';

export function useProbeResults(opts?: { enabled?: boolean; modelId?: string }) {
  return useQuery({
    queryKey: ['probe-results', opts?.modelId ?? 'all'],
    queryFn: () => api<ProbeResult[]>(opts?.modelId ? `/probe-results?model_id=${encodeURIComponent(opts.modelId)}` : '/probe-results'),
    refetchInterval: 30_000,
    enabled: opts?.enabled !== false,
  });
}
