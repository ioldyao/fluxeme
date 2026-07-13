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

export interface DeductionResponse {
  items: DeductionRecord[];
  total: number;
}

export function useDeductions(year: number, month: number, page?: number, size?: number) {
  const params = new URLSearchParams({ year: String(year), month: String(month) });
  if (page != null && size != null) {
    params.set('limit', String(size));
    params.set('offset', String((page - 1) * size));
  }
  return useQuery({
    queryKey: ['billing', 'deductions', year, month, page, size],
    queryFn: () => api<DeductionResponse>(`/billing/deductions?${params}`),
  });
}

export function useBillingMonths() {
  return useQuery({
    queryKey: ['billing', 'months'],
    queryFn: () => api<string[]>('/billing/months'),
    staleTime: 60_000,
  });
}

export function usePeriodSummaryAll(enabled?: boolean) {
  return useQuery({
    queryKey: ['billing', 'period-summary-all'],
    queryFn: () => api<MonthSummary[]>('/billing/period-summary-all'),
    enabled,
    staleTime: 60_000,
  });
}

export interface MonthSummary {
  month: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
}
