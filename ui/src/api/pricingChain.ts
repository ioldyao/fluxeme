import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { ContractPrice, TenantDiscount } from '@/types';

export function useContractPrices() {
  return useQuery({
    queryKey: ['contract-prices'],
    queryFn: () => api<ContractPrice[]>('/contract-prices'),
  });
}

export function useCreateContractPrice() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<ContractPrice>) =>
      api<ContractPrice>('/contract-prices', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['contract-prices'] }),
  });
}

export function useDeleteContractPrice() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/contract-prices/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['contract-prices'] }),
  });
}

export function useTenantDiscounts() {
  return useQuery({
    queryKey: ['tenant-discounts'],
    queryFn: () => api<TenantDiscount[]>('/tenant-discounts'),
  });
}

export function useCreateTenantDiscount() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<TenantDiscount>) =>
      api<TenantDiscount>('/tenant-discounts', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tenant-discounts'] }),
  });
}

export function useDeleteTenantDiscount() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/tenant-discounts/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tenant-discounts'] }),
  });
}
