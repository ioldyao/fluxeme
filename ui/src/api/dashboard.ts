import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import { useTimezone } from '@/store/timezone';
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
  const tz = useTimezone((s) => s.timezone);
  return useQuery({
    queryKey: ['usage', 'daily', days, tz],
    queryFn: () => api<DailyUsage[]>(`/usage/daily?limit=${days}&tz=${encodeURIComponent(tz)}`),
    refetchInterval: 60_000,
  });
}
