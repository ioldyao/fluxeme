// Importers/callers: used by ui/src/pages/Dashboard.tsx and admin observability
// views like ui/src/pages/FlowTowerContent.tsx. Affected APIs: GET /dashboard,
// GET /dashboard/self, GET /dashboard/aggregations, and GET
// /dashboard/self/aggregations. Data schemas: DashboardStats,
// DashboardAggregations, DailyUsage, and SelfDashboardStats { api_keys,
// total_requests }. User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
import { useQuery } from '@tanstack/react-query';
import { useAuth } from '@shared/store/auth';
import { api } from './client';
import type { DashboardStats, DashboardAggregations, DailyUsage } from '@shared/types';

export interface SelfDashboardStats {
  api_keys: number;
  total_requests: number;
}

export function useDashboard() {
  return useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api<DashboardStats>('/dashboard'),
    refetchInterval: 60_000,
  });
}

export function useSelfDashboard() {
  const userId = useAuth(state => state.userId);

  return useQuery({
    queryKey: ['dashboard', 'self', userId],
    queryFn: () => api<SelfDashboardStats>('/dashboard/self'),
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

export function useSelfDashboardAggregations() {
  const userId = useAuth(state => state.userId);

  return useQuery({
    queryKey: ['dashboard', 'self', userId, 'aggregations'],
    queryFn: () => api<DashboardAggregations>('/dashboard/self/aggregations'),
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
