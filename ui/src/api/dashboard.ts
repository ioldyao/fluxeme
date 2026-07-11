import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { DashboardStats, DashboardAggregations, DailyUsage } from '@/types';

export function useDashboard() {
  return useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api<DashboardStats>('/dashboard'),
    refetchInterval: 60_000,
  });
}

export function useDashboardAggregations() {
  return useQuery({
    queryKey: ['dashboard', 'aggregations'],
    queryFn: () => api<DashboardAggregations>('/dashboard/aggregations'),
    refetchInterval: 60_000,
  });
}

export function useDailyUsage(days = 14) {
  return useQuery({
    queryKey: ['usage', 'daily', days],
    queryFn: () => api<DailyUsage[]>(`/usage/daily?limit=${days}`),
    refetchInterval: 60_000,
  });
}
