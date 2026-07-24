import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { ProbeResult } from '@/types';

export function useProbeResults(opts?: { enabled?: boolean }) {
  return useQuery({
    queryKey: ['probe-results'],
    queryFn: () => api<ProbeResult[]>('/probe-results'),
    refetchInterval: 30_000,
    enabled: opts?.enabled !== false,
  });
}
