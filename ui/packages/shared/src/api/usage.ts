import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useAuth } from '../store/auth';
import { api } from './client';
import type { UsageRecord, DailyAggregate, ModelActivity } from '../types';

interface UsageParams {
  limit?: number;
  offset?: number;
  user_id?: string;
  model?: string;
  api_key?: string;
  api_format?: string;
  start_date?: string;
  end_date?: string;
}

interface UsageResponse {
  records: UsageRecord[];
  total: number;
}

export interface UsageBillingRow {
  request_id: string;
  package_units: number;
  wallet_amount: number;
  wallet_debit_status: 'charged' | 'no_charge' | 'pending' | 'unavailable';
  account_type?: string | null;
  billing_payment_mode?: 'metered' | 'prepaid' | null;
}

function buildUsageSearchParams(params: UsageParams = {}) {
  const searchParams = new URLSearchParams();
  if (params.limit) searchParams.set('limit', String(params.limit));
  if (params.offset) searchParams.set('offset', String(params.offset));
  if (params.user_id) searchParams.set('user_id', params.user_id);
  if (params.model) searchParams.set('model', params.model);
  if (params.api_key) searchParams.set('api_key', params.api_key);
  if (params.api_format) searchParams.set('api_format', params.api_format);
  if (params.start_date) searchParams.set('start_date', params.start_date);
  if (params.end_date) searchParams.set('end_date', params.end_date);
  return searchParams;
}

export function useUsage(params: UsageParams = {}) {
  const qs = buildUsageSearchParams(params).toString();

  // Serialize to prevent object-reference instability causing infinite refetch
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['usage', 'all', stableKey],
    queryFn: () => api<UsageResponse>(`/usage${qs ? `?${qs}` : ''}`),
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  });
}

export function useMyUsage(params: Omit<UsageParams, 'user_id'> = {}) {
  const userId = useAuth(state => state.userId);
  const qs = buildUsageSearchParams(params).toString();
  const stableKey = JSON.stringify(params);

  return useQuery({
    queryKey: ['usage', 'self', userId, stableKey],
    queryFn: () => api<UsageResponse>(`/me/usage${qs ? `?${qs}` : ''}`),
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  });
}

export function useMyUsageBilling(requestIds: string[]) {
  const userId = useAuth(state => state.userId);
  const stableRequestIds = [...requestIds].sort();
  const query = new URLSearchParams();
  query.set('request_ids', stableRequestIds.join(','));

  return useQuery({
    queryKey: ['usage', 'billing', 'self', userId, stableRequestIds],
    queryFn: () => api<UsageBillingRow[]>(`/me/usage/billing?${query.toString()}`),
    enabled: !!userId && stableRequestIds.length > 0,
    refetchInterval: 10_000,
  });
}

export function useAdminUsageBilling(requestIds: string[]) {
  const stableRequestIds = [...requestIds].sort();
  const query = new URLSearchParams();
  query.set('request_ids', stableRequestIds.join(','));
  return useQuery({
    queryKey: ['usage', 'billing', 'admin', stableRequestIds],
    queryFn: () => api<UsageBillingRow[]>(`/admin/usage/billing?${query.toString()}`),
    enabled: stableRequestIds.length > 0,
  });
}

export function useUsageDetail(requestId: string | null) {
  return useQuery({
    queryKey: ['usage', requestId],
    queryFn: () => api<UsageRecord>(`/usage/${requestId}`),
    enabled: !!requestId,
  });
}

// Importers/callers: used by ui/src/pages/Dashboard.tsx and ui/src/pages/Usage.tsx
// to load usage trend data. Affected APIs: GET /usage/aggregate with optional
// query param user_id for shared admin views, and GET /me/usage/aggregate for the
// self-only dashboard. Data schema: DailyAggregate[] where each row is { date,
// count, prompt_tokens, completion_tokens, total_tokens, success_count,
// latency_ms, cache_hit_tokens }. User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
export function useUsageAggregate(days: number = 14) {
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'aggregate', 'all', days],
    queryFn: () => api<DailyAggregate[]>(`/usage/aggregate?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}

export function useMyUsageAggregate(days: number = 14) {
  const userId = useAuth(state => state.userId);
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'aggregate', 'self', userId, days],
    queryFn: () => api<DailyAggregate[]>(`/me/usage/aggregate?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}

export interface FunnelStats {
  total: number;
  success_count: number;
  auth_fail_count: number;
  rate_limit_count: number;
  bad_request_count: number;
  upstream_error_count: number;
  timeout_count: number;
  other_error_count: number;
  p50_latency: number;
  p95_latency: number;
  p99_latency: number;
  avg_latency: number;
}

// Importers/callers: used by ui/src/pages/Dashboard.tsx to load request-funnel
// metrics and may be reused by other analytics views. Affected APIs: GET
// /usage/funnel for shared views and GET /me/usage/funnel for the self-only
// dashboard. Data schema: FunnelStats { total, success_count, auth_fail_count,
// rate_limit_count, bad_request_count, upstream_error_count, timeout_count,
// other_error_count, p50_latency, p95_latency, p99_latency, avg_latency }.
// User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
export function useUsageFunnel(days: number) {
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'funnel', 'all', days],
    queryFn: () => api<FunnelStats>(`/usage/funnel?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}

export function useMyUsageFunnel(days: number) {
  const userId = useAuth(state => state.userId);
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'funnel', 'self', userId, days],
    queryFn: () => api<FunnelStats>(`/me/usage/funnel?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}

// Importers/callers: used by ui/src/pages/Dashboard.tsx and ui/src/pages/Usage.tsx
// to load per-model usage summaries. Affected APIs: GET /usage/model-activity
// for shared views and GET /me/usage/model-activity for the self-only dashboard.
// Data schema: ModelActivity[] where each row is { model, total_requests,
// prompt_tokens, completion_tokens, cache_hit_tokens, success_count,
// failure_count }. User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
export function useModelActivity(days: number = 7) {
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'model-activity', 'all', days],
    queryFn: () => api<ModelActivity[]>(`/usage/model-activity?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}

export function useMyModelActivity(days: number = 7) {
  const userId = useAuth(state => state.userId);
  const searchParams = new URLSearchParams({ days: String(days) });

  return useQuery({
    queryKey: ['usage', 'model-activity', 'self', userId, days],
    queryFn: () => api<ModelActivity[]>(`/me/usage/model-activity?${searchParams.toString()}`),
    refetchInterval: 60_000,
  });
}
