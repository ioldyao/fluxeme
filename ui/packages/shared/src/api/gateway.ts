import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { GatewayRoute } from '../types';

export interface GatewayRouteInput {
  id?: string;
  name: string;
  path_prefix: string;
  upstream_url: string;
  methods: string;
  timeout_ms: number;
  enabled: boolean;
  preserve_query: boolean;
  strip_prefix: boolean;
  upstream_headers: Record<string, string>;
}

const gatewayRoutesKey = ['admin', 'gateway-routes'];

export function useGatewayRoutes() {
  return useQuery({ queryKey: gatewayRoutesKey, queryFn: () => api<GatewayRoute[]>('/gateway/routes') });
}

export function useCreateGatewayRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: GatewayRouteInput) => api<GatewayRoute>('/gateway/routes', { method: 'POST', body: input }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: gatewayRoutesKey }),
  });
}

export function useUpdateGatewayRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, input }: { id: string; input: GatewayRouteInput }) => api<GatewayRoute>(`/gateway/routes/${encodeURIComponent(id)}`, { method: 'PUT', body: input }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: gatewayRoutesKey }),
  });
}

export function useDeleteGatewayRoute() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => api<void>(`/gateway/routes/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: gatewayRoutesKey }),
  });
}
