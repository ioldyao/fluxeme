import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { Channel } from '@/types';

export function useChannels() {
  return useQuery({
    queryKey: ['channels'],
    queryFn: () => api<Channel[]>('/channels'),
  });
}

export function useCreateChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<Channel>) =>
      api<Channel>('/channels', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['channels'] }),
  });
}

export function useUpdateChannel(id: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: Partial<Channel>) =>
      api<Channel>(`/channels/${encodeURIComponent(id)}`, { method: 'PUT', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['channels'] }),
  });
}

export function useDeleteChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<void>(`/channels/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['channels'] }),
  });
}
