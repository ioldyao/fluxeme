import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';

export interface WalletOverview {
  balance: number;
  frozen: number;
  total_consumed: number;
  total_recharged: number;
}

export interface TokenPackageGrant {
  id: string;
  plan_id: string;
  user_id: string | null;
  team_id: string | null;
  accounting_mode: 'raw_tokens' | 'standardized_credits';
  display_token_amount: number;
  total_units: number;
  consumed_units: number;
  reserved_units: number;
  priority: number;
  exhaustion_policy: 'package_then_wallet' | 'package_only';
  status: string;
  expires_at: string | null;
  created_at: string;
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
  total_dates: number;
}

export interface RechargeKeyRow {
  key: string;
  amount: number;
  used_by: string | null;
  used_at: string | null;
  created_by: string;
  created_at: string;
  expires_at: string | null;
  revoked: boolean;
  /** Team scope. Present when the key is for team recharge. */
  team_id?: string | null;
}

export function useTokenPackageGrants() {
  return useQuery({
    queryKey: ['token-packages', 'mine'],
    queryFn: () => api<TokenPackageGrant[]>('/me/token-packages'),
    refetchInterval: 30_000,
  });
}

export function useWalletOverview() {
  return useQuery({
    queryKey: ['wallet', 'overview'],
    queryFn: () => api<WalletOverview>('/wallet/overview'),
    refetchInterval: 30_000,
  });
}

export function useWalletTransactions(
  page: number,
  size: number,
  filters?: { since?: string; until?: string; tx_type?: string },
) {
  const params = new URLSearchParams({ page: String(page), size: String(size) });
  if (filters?.since) params.set('since', filters.since);
  if (filters?.until) params.set('until', filters.until);
  if (filters?.tx_type) params.set('tx_type', filters.tx_type);

  return useQuery({
    queryKey: ['wallet', 'transactions', page, size, filters],
    queryFn: () => api<WalletTxResponse>(`/wallet/transactions?${params}`),
  });
}

export interface RechargeKeyResponse {
  items: RechargeKeyRow[];
  total: number;
}

export function useRechargeKeys(
  page?: number,
  size?: number,
  filters?: { search?: string; status?: string; used_by?: string },
  options?: { enabled?: boolean },
) {
  const params = new URLSearchParams();
  if (page != null && size != null) {
    params.set('limit', String(size));
    params.set('offset', String((page - 1) * size));
  }
  if (filters?.search) params.set('search', filters.search);
  if (filters?.status) params.set('status', filters.status);
  if (filters?.used_by) params.set('used_by', filters.used_by);
  return useQuery({
    queryKey: ['wallet', 'keys', page, size, filters],
    queryFn: () => api<RechargeKeyResponse>(`/wallet/keys?${params}`),
    staleTime: 10_000,
    enabled: options?.enabled ?? true,
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
    mutationFn: (data: { amount: number; expires_at?: string; team_id?: string }) =>
      api<{ key: string; amount: number; expires_at?: string; team_id?: string }>('/wallet/create-key', {
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['wallet', 'keys'] });
    },
  });
}

export function useRevokeKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (key: string) =>
      api<{ success: boolean }>('/wallet/revoke-key', {
        method: 'POST',
        body: { key },
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
      api<{ amount: number; balance: number; team_id?: string }>('/wallet/redeem-key', {
        method: 'POST',
        body: { key },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['wallet'] });
    },
  });
}
