import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { DashboardStats, DashboardAggregations } from '@/types';

export function useDashboard() {
  return useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api<DashboardStats>('/dashboard'),
  });
}

export function useDashboardAggregations() {
  return useQuery({
    queryKey: ['dashboard', 'aggregations'],
    queryFn: () => api<DashboardAggregations>('/dashboard/aggregations'),
    refetchInterval: 60_000,
  });
}
