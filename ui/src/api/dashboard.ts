import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { DashboardStats } from '@/types';

export function useDashboard() {
  return useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api<DashboardStats>('/dashboard'),
  });
}
