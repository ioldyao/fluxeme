import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';

export interface WalletOverview {
  balance: number;
  frozen: number;
  total_consumed: number;
  total_recharged: number;
}

export interface WalletTransaction {
  id: string;
  tx_type: string;
  amount: number;
  balance_before: number;
  balance_after: number;
  method: string;
  status: string;
  note: string;
  created_at: string;
}

export interface WalletTxResponse {
  items: WalletTransaction[];
  total: number;
}

export interface RechargeKeyRow {
  key: string;
  amount: number;
  used_by: string | null;
  used_at: string | null;
  created_by: string;
  created_at: string;
}

export function useWalletOverview() {
  return useQuery({
    queryKey: ['wallet', 'overview'],
    queryFn: () => api<WalletOverview>('/wallet/overview'),
    refetchInterval: 30_000,
  });
}

export function useWalletTransactions(page: number, size: number) {
  return useQuery({
    queryKey: ['wallet', 'transactions', page, size],
    queryFn: () => api<WalletTxResponse>(`/wallet/transactions?page=${page}&size=${size}`),
  });
}

export function useRechargeKeys() {
  return useQuery({
    queryKey: ['wallet', 'keys'],
    queryFn: () => api<RechargeKeyRow[]>('/wallet/keys'),
    staleTime: 10_000,
  });
}

export function useEstimatedDays() {
  return useQuery({
    queryKey: ['wallet', 'estimated-days'],
    queryFn: () => api<{ days: number | null }>('/wallet/estimated-days'),
    staleTime: 60_000,
  });
}

export function useRecharge() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (amount: number) =>
      api<{ transaction_id: string; amount: number; balance: number }>('/wallet/recharge', {
        method: 'POST',
        body: { amount },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['wallet'] });
    },
  });
}

export function useCreateRechargeKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (amount: number) =>
      api<{ key: string; amount: number }>('/wallet/create-key', {
        method: 'POST',
        body: { amount },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['wallet', 'keys'] });
    },
  });
}

export function useRedeemKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (key: string) =>
      api<{ amount: number; balance: number }>('/wallet/redeem-key', {
        method: 'POST',
        body: { key },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['wallet'] });
    },
  });
}
