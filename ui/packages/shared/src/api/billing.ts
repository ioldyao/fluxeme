import { useQueries, useQuery } from '@tanstack/react-query';
import { api } from './client';
import type {
  AdminBillingActivity,
  AdminBillingApiKeyActivityResponse,
  AdminBillingApiKeyDetailResponse,
  AdminBillingSummary,
  AdminBillingTeamSpendRankingResponse,
  AdminBillingTeamsResponse,
  AdminBillingTeamUsersResponse,
  AdminBillingTrendPoint,
  AdminBillingUserApiKeyCostResponse,
  AdminBillingUserSpendRankingResponse,
  BillingSummary,
  UsageRecord,
} from '../types';

export interface BillingActivity {
  timestamp: string;
  request_id: string;
  user_id?: string;
  user_name?: string;
  model: string;
  channel_id: string;
  activity_status: string;
  status_reason: string;
  status_code: number;
  success: boolean;
  prompt_tokens: number;
  completion_tokens: number;
  cache_hit_input_tokens: number;
  cache_write_tokens: number;
  total_tokens: number;
  package_units: number;
  package_grant_id?: string | null;
  wallet_amount: number;
  priced_cost_amount: number;
  charge_source: string;
  account_type: string;
  team_id?: string | null;
  api_key_name?: string | null;
  latency_ms: number;
  reservation_id?: string | null;
}

export interface BillingActivityDimensionRow {
  name: string;
  activity_count: number;
  key_count: number;
  model_count: number;
  related_names: string[];
  source_names: string[];
  total_tokens: number;
  package_units: number;
  wallet_amount: number;
  priced_cost_amount: number;
}

export interface BillingActivityDimensions {
  api_keys: BillingActivityDimensionRow[];
  models: BillingActivityDimensionRow[];
  sources: BillingActivityDimensionRow[];
}

export interface BillingActivitySummary {
  activity_count: number;
  success_count: number;
  failed_count: number;
  interrupted_count: number;
  zero_cost_count: number;
  total_tokens: number;
  package_units: number;
  wallet_amount: number;
  priced_cost_amount: number;
  api_key_count: number;
  model_count: number;
}

export interface BillingActivityResponse {
  activities: BillingActivity[];
  total: number;
  summary: BillingActivitySummary;
  dimensions: BillingActivityDimensions;
}

export interface BillingActivityFilters {
  search?: string;
  api_key_name?: string;
  model?: string;
  charge_source?: string;
}

function activityQuery(filters: BillingActivityFilters): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) {
    if (value && value !== 'all') params.set(key, value);
  }
  return params.toString();
}

export function useBillingActivities(year: number, month: number, limit = 50, offset = 0, filters: BillingActivityFilters = {}) {
  const query = activityQuery(filters);
  return useQuery({
    queryKey: ['billing', 'activities', year, month, limit, offset, query],
    queryFn: () => api<BillingActivityResponse>(`/billing/activities?year=${year}&month=${month}&limit=${limit}&offset=${offset}${query ? `&${query}` : ''}`),
  });
}

export function useAdminBillingActivities(year: number, month: number, limit = 100, offset = 0, userId?: string | null, filters: BillingActivityFilters = {}) {
  const query = activityQuery(filters);
  return useQuery({
    queryKey: ['admin-billing', 'activities', year, month, limit, offset, userId, query],
    queryFn: () => api<BillingActivityResponse>(`/admin/billing/activities?year=${year}&month=${month}&limit=${limit}&offset=${offset}${userId ? `&user_id=${encodeURIComponent(userId)}` : ''}${query ? `&${query}` : ''}`),
  });
}

export interface PeriodSummary {
  year: number;
  month: number;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  by_model: { model: string; cost: number; percentage: number }[];
  by_channel: { channel: string; name: string; cost: number; percentage: number }[];
  token_cost_breakdown: { token_type: string; total_tokens: number; total_cost: number; percentage: number }[];
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

export function useAdminBillingSummary() {
  return useQuery({
    queryKey: ['admin-billing', 'summary'],
    queryFn: () => api<AdminBillingSummary>('/admin/billing/summary'),
    refetchInterval: 60_000,
  });
}

export function useAdminBillingActivity(year: number, month: number, enabled = true) {
  return useQuery({
    queryKey: ['admin-billing', 'activity', year, month],
    queryFn: () => api<AdminBillingActivity>(`/admin/billing/active?year=${year}&month=${month}`),
    enabled,
  });
}

export function useAdminBillingTeamSpendRanking(year: number, month: number, limit = 10, enabled = true) {
  return useQuery({
    queryKey: ['admin-billing', 'team-ranking', year, month, limit],
    queryFn: () => api<AdminBillingTeamSpendRankingResponse>(`/admin/billing/team-spend-ranking?year=${year}&month=${month}&limit=${limit}`),
    enabled,
  });
}

export function usePeriodSummary(year: number, month: number) {
  return useQuery({
    queryKey: ['billing', 'period', year, month],
    queryFn: () => api<PeriodSummary>(`/billing/period-summary?year=${year}&month=${month}`),
  });
}

export function useAdminPeriodSummary(year: number, month: number, enabled = true) {
  return useQuery({
    queryKey: ['admin-billing', 'period', year, month],
    queryFn: () => api<PeriodSummary>(`/admin/billing/period-summary?year=${year}&month=${month}`),
    enabled,
  });
}

export function useAdminScopedPeriodSummary(
  year: number,
  month: number,
  params: { team_id?: string | null; user_id?: string | null } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({ year: String(year), month: String(month) });
  if (params.team_id) searchParams.set('team_id', params.team_id);
  if (params.user_id) searchParams.set('user_id', params.user_id);

  return useQuery({
    queryKey: ['admin-billing', 'scoped-period', year, month, JSON.stringify(params)],
    queryFn: () => api<PeriodSummary>(`/admin/billing/scoped-period-summary?${searchParams.toString()}`),
    enabled,
  });
}

export function useAdminBillingDailyTrend(
  year: number,
  month: number,
  params: { team_id?: string | null; user_id?: string | null } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({ year: String(year), month: String(month) });
  if (params.team_id) searchParams.set('team_id', params.team_id);
  if (params.user_id) searchParams.set('user_id', params.user_id);

  return useQuery({
    queryKey: ['admin-billing', 'daily-trend', year, month, JSON.stringify(params)],
    queryFn: () => api<AdminBillingTrendPoint[]>(`/admin/billing/daily-trend?${searchParams.toString()}`),
    enabled,
  });
}

export function useAdminBillingUserSpendRanking(year: number, month: number, limit = 10, enabled = true) {
  return useQuery({
    queryKey: ['admin-billing', 'user-ranking', year, month, limit],
    queryFn: () => api<AdminBillingUserSpendRankingResponse>(`/admin/billing/user-spend-ranking?year=${year}&month=${month}&limit=${limit}`),
    enabled,
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

export function useAdminDeductions(
  year: number,
  month: number,
  page?: number,
  size?: number,
  params: { team_id?: string | null; user_id?: string | null } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({ year: String(year), month: String(month) });
  if (page != null && size != null) {
    searchParams.set('limit', String(size));
    searchParams.set('offset', String((page - 1) * size));
  }
  if (params.team_id) searchParams.set('team_id', params.team_id);
  if (params.user_id) searchParams.set('user_id', params.user_id);
  return useQuery({
    queryKey: ['admin-billing', 'deductions', year, month, page, size, JSON.stringify(params)],
    queryFn: () => api<DeductionResponse>(`/admin/billing/deductions?${searchParams.toString()}`),
    enabled,
  });
}

export function useBillingMonths() {
  return useQuery({
    queryKey: ['billing', 'months'],
    queryFn: () => api<string[]>('/billing/months'),
    staleTime: 60_000,
  });
}

export function useAdminBillingMonths() {
  return useQuery({
    queryKey: ['admin-billing', 'months'],
    queryFn: () => api<string[]>('/admin/billing/months'),
    staleTime: 60_000,
  });
}

export function usePeriodSummaryAll() {
  return useQuery({
    queryKey: ['billing', 'period-summary-all'],
    queryFn: () => api<MonthSummary[]>('/billing/period-summary-all'),
    staleTime: 60_000,
  });
}

export function useAdminPeriodSummaryAll(
  params: { team_id?: string | null; user_id?: string | null } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams();
  if (params.team_id) searchParams.set('team_id', params.team_id);
  if (params.user_id) searchParams.set('user_id', params.user_id);
  const query = searchParams.toString();
  return useQuery({
    queryKey: ['admin-billing', 'period-summary-all', JSON.stringify(params)],
    queryFn: () => api<MonthSummary[]>(`/admin/billing/period-summary-all${query ? `?${query}` : ''}`),
    staleTime: 60_000,
    enabled,
  });
}

export function useAdminBillingTeams(
  year: number,
  month: number,
  params: { limit?: number; offset?: number; search?: string; sort_by?: string; sort_order?: string } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 20),
    offset: String(params.offset ?? 0),
  });
  if (params.search) searchParams.set('search', params.search);
  if (params.sort_by) searchParams.set('sort_by', params.sort_by);
  if (params.sort_order) searchParams.set('sort_order', params.sort_order);

  return useQuery({
    queryKey: ['admin-billing', 'teams', year, month, JSON.stringify(params)],
    queryFn: () => api<AdminBillingTeamsResponse>(`/admin/billing/teams?${searchParams.toString()}`),
    enabled,
  });
}

export function useAdminBillingTeamUsers(teamId: string | null, year: number, month: number, params: { limit?: number; offset?: number } = {}, enabled = true) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 20),
    offset: String(params.offset ?? 0),
  });

  return useQuery({
    queryKey: ['admin-billing', 'team-users', teamId, year, month, JSON.stringify(params)],
    queryFn: () => api<AdminBillingTeamUsersResponse>(`/admin/billing/teams/${teamId}/users?${searchParams.toString()}`),
    enabled: enabled && !!teamId,
  });
}

export function useAdminBillingTeamUserApiKeys(teamId: string | null, userId: string | null, year: number, month: number, params: { limit?: number; offset?: number; model?: string; api_format?: string } = {}, enabled = true) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 50),
    offset: String(params.offset ?? 0),
  });
  if (params.model) searchParams.set('model', params.model);
  if (params.api_format) searchParams.set('api_format', params.api_format);

  return useQuery({
    queryKey: ['admin-billing', 'team-user-api-keys', teamId, userId, year, month, JSON.stringify(params)],
    queryFn: () => api<AdminBillingApiKeyActivityResponse>(`/admin/billing/teams/${teamId}/users/${userId}/api-keys?${searchParams.toString()}`),
    enabled: enabled && !!teamId && !!userId,
  });
}

export function useAdminBillingTeamUsersApiKeys(
  teamId: string | null,
  users: Array<{ userId: string; userName: string }>,
  year: number,
  month: number,
  params: { limit?: number; offset?: number; model?: string; api_format?: string } = {},
  enabled = true,
) {
  return useQueries({
    queries: users.map((user) => {
      const searchParams = new URLSearchParams({
        year: String(year),
        month: String(month),
        limit: String(params.limit ?? 50),
        offset: String(params.offset ?? 0),
      });
      if (params.model) searchParams.set('model', params.model);
      if (params.api_format) searchParams.set('api_format', params.api_format);

      return {
        queryKey: ['admin-billing', 'team-user-api-keys', teamId, user.userId, year, month, JSON.stringify(params)],
        queryFn: () => api<AdminBillingApiKeyActivityResponse>(`/admin/billing/teams/${teamId}/users/${user.userId}/api-keys?${searchParams.toString()}`),
        enabled: enabled && !!teamId && !!user.userId,
      };
    }),
  });
}

export function useAdminBillingTeamRequests(
  teamId: string | null,
  year: number,
  month: number,
  params: { limit?: number; offset?: number; user_id?: string; api_key_name?: string; model?: string; api_format?: string } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 50),
    offset: String(params.offset ?? 0),
  });
  if (params.user_id) searchParams.set('user_id', params.user_id);
  if (params.api_key_name) searchParams.set('api_key_name', params.api_key_name);
  if (params.model) searchParams.set('model', params.model);
  if (params.api_format) searchParams.set('api_format', params.api_format);

  return useQuery({
    queryKey: ['admin-billing', 'team-requests', teamId, year, month, JSON.stringify(params)],
    queryFn: () => api<{ records: import('../types').UsageRecord[]; total: number }>(`/admin/billing/teams/${teamId}/requests?${searchParams.toString()}`),
    enabled: enabled && !!teamId,
  });
}

export function useAdminBillingUserApiKeyCosts(
  teamId: string | null,
  userId: string | null,
  year: number,
  month: number,
  params: { limit?: number; offset?: number } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 20),
    offset: String(params.offset ?? 0),
  });

  return useQuery({
    queryKey: ['admin-billing', 'user-api-key-costs', teamId, userId, year, month, JSON.stringify(params)],
    queryFn: () => {
      if (!userId) {
        throw new Error('userId is required');
      }
      const path = teamId
        ? `/admin/billing/teams/${teamId}/users/${userId}/api-key-costs`
        : `/admin/billing/users/${userId}/api-key-costs`;
      return api<AdminBillingUserApiKeyCostResponse>(`${path}?${searchParams.toString()}`);
    },
    enabled: enabled && !!userId,
  });
}

export function useAdminBillingApiKeyDetail(
  teamId: string | null,
  userId: string | null,
  apiKeyName: string | null,
  year: number,
  month: number,
  params: { limit?: number; offset?: number; api_format?: string } = {},
  enabled = true,
) {
  const searchParams = new URLSearchParams({
    year: String(year),
    month: String(month),
    limit: String(params.limit ?? 20),
    offset: String(params.offset ?? 0),
  });
  if (params.api_format) searchParams.set('api_format', params.api_format);

  return useQuery({
    queryKey: ['admin-billing', 'api-key-detail', teamId, userId, apiKeyName, year, month, JSON.stringify(params)],
    queryFn: () => {
      if (!userId || !apiKeyName) {
        throw new Error('userId and apiKeyName are required');
      }
      const path = teamId
        ? `/admin/billing/teams/${teamId}/users/${userId}/api-keys/${encodeURIComponent(apiKeyName)}`
        : `/admin/billing/users/${userId}/api-keys/${encodeURIComponent(apiKeyName)}`;
      return api<AdminBillingApiKeyDetailResponse>(`${path}?${searchParams.toString()}`);
    },
    enabled: enabled && !!userId && !!apiKeyName,
  });
}

export function useAdminBillingRequestDetail(requestId: string | null, enabled = true) {
  return useQuery({
    queryKey: ['admin-billing', 'request-detail', requestId],
    queryFn: () => api<UsageRecord>(`/admin/billing/requests/${requestId}`),
    enabled: enabled && !!requestId,
  });
}

export interface MonthSummary {
  month: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
}
