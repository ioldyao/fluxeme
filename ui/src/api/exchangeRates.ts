import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { ExchangeRateRow } from '@/types';

export function useExchangeRates(enabled?: boolean) {
  return useQuery<ExchangeRateRow[]>({
    queryKey: ['exchange-rates'],
    queryFn: () => api('/exchange-rates'),
    enabled,
    refetchInterval: 60_000,
  });
}

export function useUpsertExchangeRate() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: { base_currency?: string; quote_currency: string; rate: number; rate_date?: string; source?: string; notes?: string }) =>
      api('/exchange-rates', { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['exchange-rates'] }),
  });
}

export function useRefreshExchangeRates() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => api<{ ok: boolean; count: number }>('/exchange-rates/refresh', { method: 'POST' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['exchange-rates'] }),
  });
}

export async function fetchLatestRates(): Promise<ExchangeRateRow[]> {
  return api<ExchangeRateRow[]>('/exchange-rates/latest');
}

export async function fetchUsdToCnyRate(): Promise<number> {
  const rates = await fetchLatestRates();
  const cnyRate = rates.find(r => r.base_currency === 'USD' && r.quote_currency === 'CNY');
  return cnyRate?.rate ?? 7.2;
}
