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
  request_id?: string;
  channel_name?: string;
  channel_id?: string;
  endpoint_url?: string;
  client_ip?: string;
}

interface UsageResponse {
  records: UsageRecord[];
  total: number;
}

export interface UsageRequest {
  timestamp: string | number; request_id: string; user_id?: string | null; user_name?: string | null; team_id?: string | null; api_key_name?: string | null; api_format: string; stream: number; client_ip?: string | null; requested_model: string; resolved_model?: string | null; model?: string; success?: boolean; latency_ms?: number; cache_hit_input_tokens?: number; channel_id?: string | null; endpoint_id?: number | null; endpoint_url?: string | null; status: string; status_code: number; error_stage?: string | null; error_kind?: string | null; error_message?: string | null; attempt_count: number; successful_attempt?: number | null; prompt_tokens: number; completion_tokens: number; cache_read_tokens: number; cache_write_tokens: number; total_tokens: number; total_latency_ms: number; ttft_ms?: number | null; client_disconnected: number; billing_payment_mode?: string | null;
}
export interface UsageRequestAttempt { timestamp?: string | number; attempt_no: number; channel_id?: string | null; endpoint_id?: number | null; endpoint_url?: string | null; provider?: string | null; status_code?: number | null; success: number; timeout: number; error?: string | null; latency_ms: number; }
export interface UsageRequestParams extends UsageParams { request_id?: string; channel_id?: string; endpoint_url?: string; client_ip?: string; }
export function useUsageRequests(params: UsageRequestParams = {}) { const query = new URLSearchParams(); Object.entries(params).forEach(([k,v]) => { if (v !== undefined && v !== '') query.set(k, String(v)); }); const key = JSON.stringify(params); return useQuery({ queryKey: ['usage','requests',key], queryFn: () => api<{records: UsageRequest[]; total: number}>(`/admin/usage/requests?${query}`), placeholderData: keepPreviousData, refetchInterval: 60_000 }); }
export function useUsageRequestAttempts(requestId: string | null) { return useQuery({ queryKey: ['usage','requests',requestId,'attempts'], queryFn: () => api<UsageRequestAttempt[]>(`/admin/usage/requests/${encodeURIComponent(requestId!)}/attempts`), enabled: !!requestId }); }
export function useUsageRequestDetail(requestId: string | null) { return useQuery({ queryKey: ['usage','requests',requestId], queryFn: () => api<UsageRequest>(`/admin/usage/requests/${encodeURIComponent(requestId!)}`), enabled: !!requestId }); }

export interface UsageBillingRow {
  request_id: string;
  package_units: number;
  wallet_amount: number;
  wallet_debit_status: 'charged' | 'no_charge' | 'pending' | 'unavailable';
  account_type?: string | null;
  billing_payment_mode?: 'metered' | 'prepaid' | null;
}

export interface UsageAnalyticsBucket {
  date: string;
  requests: number;
  succeeded: number;
  failed: number;
  input_tokens: number;
  cache_read_tokens: number;
  output_tokens: number;
  total_tokens: number;
  latency_ms: number;
}

export interface UsageAnalyticsModel {
  model: string;
  requests: number;
  succeeded: number;
  failed: number;
  input_tokens: number;
  cache_read_tokens: number;
  output_tokens: number;
}

export interface UsageAnalyticsTotals {
  requests: number;
  succeeded: number;
  failed: number;
  input_tokens: number;
  cache_read_tokens: number;
  output_tokens: number;
  total_tokens: number;
  latency_ms: number;
}

export interface UsageAnalyticsResponse {
  schema_version: number;
  days: number;
  buckets: UsageAnalyticsBucket[];
  totals: UsageAnalyticsTotals;
  models: UsageAnalyticsModel[];
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
  if (params.request_id) searchParams.set('request_id', params.request_id);
  if (params.channel_name) searchParams.set('channel_name', params.channel_name);
  if (params.channel_id) searchParams.set('channel_id', params.channel_id);
  if (params.endpoint_url) searchParams.set('endpoint_url', params.endpoint_url);
  if (params.client_ip) searchParams.set('client_ip', params.client_ip);
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

export function useMyUsageAnalytics(days: number = 7, enabled = true) {
  const userId = useAuth(state => state.userId);
  const safeDays = Math.min(30, Math.max(1, days));

  return useQuery({
    queryKey: ['usage', 'analytics', 'self', userId, safeDays],
    queryFn: () => api<UsageAnalyticsResponse>(`/me/usage/analytics?days=${safeDays}`),
    enabled: enabled && !!userId,
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  });
}

export function useUsageAnalytics({ days = 7, start_date, end_date, enabled = true }: {
  days?: number;
  start_date?: string;
  end_date?: string;
  enabled?: boolean;
} = {}) {
  const safeDays = Math.min(30, Math.max(1, days));
  const searchParams = new URLSearchParams();
  if (start_date) searchParams.set('start_date', start_date);
  else searchParams.set('days', String(safeDays));
  if (end_date) searchParams.set('end_date', end_date);
  const qs = searchParams.toString();
  const rangeKey = JSON.stringify({ days: start_date ? undefined : safeDays, start_date, end_date });

  return useQuery({
    queryKey: ['usage', 'analytics', 'all', rangeKey],
    queryFn: () => api<UsageAnalyticsResponse>(`/usage/analytics?${qs}`),
    enabled,
    placeholderData: keepPreviousData,
    refetchInterval: 60_000,
  });
}

export function useRecentClientIps(enabled = true) {
  return useQuery({
    queryKey: ['usage', 'client-ips'],
    queryFn: () => api<string[]>('/usage/client-ips'),
    enabled,
    staleTime: 60_000,
    refetchInterval: 5 * 60_000,
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
