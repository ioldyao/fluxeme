import { useQuery } from '@tanstack/react-query';
import { api } from './client';
import type { BillingSummary } from '@/types';

export interface PeriodSummary {
  year: number;
  month: number;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  by_model: { model: string; cost: number; percentage: number }[];
  by_channel: { channel: string; cost: number; percentage: number }[];
}

export interface DeductionRecord {
  time: string;
  amount: number;
  method: string;
}

export function useBillingSummary() {
  return useQuery({
    queryKey: ['billing', 'summary'],
    queryFn: () => api<BillingSummary>('/billing/summary'),
    refetchInterval: 60_000,
  });
}

export function usePeriodSummary(year: number, month: number) {
  return useQuery({
    queryKey: ['billing', 'period', year, month],
    queryFn: () => api<PeriodSummary>(`/billing/period-summary?year=${year}&month=${month}`),
  });
}

export function useDeductions(year: number, month: number) {
  return useQuery({
    queryKey: ['billing', 'deductions', year, month],
    queryFn: () => api<DeductionRecord[]>(`/billing/deductions?year=${year}&month=${month}`),
  });
}
