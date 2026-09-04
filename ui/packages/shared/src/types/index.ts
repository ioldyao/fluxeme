export type UserRole = 'admin' | 'user';
export type UserStatus = 'active' | 'suspended';

export interface RateLimit {
  rpm: number | null;
  tpm: number | null;
}

export interface User {
  id: string;
  name: string;
  role?: string;
  rate_limits?: RateLimit | null;
  concurrency_limit?: number;
  status: UserStatus;
  suspended_at?: string | null;
  /** IdP organizations (e.g. Keycloak Organizations) the user belongs to. */
  sso_orgs?: SsoOrg[];
}

/** An organization (tenant) from the IdP, from the OIDC `organizations` claim. */
export interface SsoOrg {
  id: string;
  name?: string | null;
  alias?: string | null;
}

export interface UserDetail extends User {
  keys: ApiKey[];
}

export type BillingPaymentMode = 'metered' | 'prepaid';

export interface BillingGroup {
  id: string;
  name: string;
  payment_mode: BillingPaymentMode;
  status: string;
  is_default: boolean;
  created_by: string;
  created_at: string;
  updated_at: string;
}

export interface ApiKey {
  key: string;
  user_id: string;
  name: string;
  enabled: boolean;
  expires_at?: string | null;
  spend_limit?: number | null;
  allowed_models?: string[] | null;
  /** Team scope. Present when this is a team-shared key. */
  team_id?: string | null;
  /** 访问范围 = 资源类型（model / skill / mcp / gateway）。 */
  scopes?: string[] | null;
  billing_group_id: string;
  billing_payment_mode: BillingPaymentMode;
}

export interface GatewayRoute {
  id: string;
  name: string;
  path_prefix: string;
  upstream_url: string;
  methods: string;
  timeout_ms: number;
  enabled: boolean;
  preserve_query: boolean;
  strip_prefix: boolean;
  /** Header names only; values are never returned by the API. */
  upstream_headers: string[];
  created_at: string;
  updated_at: string;
}

export interface ManagementApiKey {
  id: string;
  key_prefix: string;
  name: string;
  enabled: boolean;
  created_by: string;
  created_at: string;
  expires_at?: string | null;
  last_used_at?: string | null;
}

export type TeamRole = 'owner' | 'admin' | 'member';

export interface Team {
  id: string;
  name: string;
  owner_id: string;
  created_at: string;
  updated_at: string;
}

export interface TeamMember {
  team_id: string;
  user_id: string;
  role: TeamRole;
  joined_at: string;
}

export interface Endpoint {
  id?: number | null;
  url: string;
  api_key: string;
  weight: number;
  timeout_secs?: number | null;
  enabled?: boolean;
  full_url?: boolean;
}

export type Provider = 'openai' | 'anthropic' | 'vllm' | 'sglang' | 'azure' | 'ollama' | string;

export interface Channel {
  id: string;
  name: string;
  provider: Provider;
  enabled: boolean;
  anthropic_compat?: boolean;
  endpoints: Endpoint[];
}

export interface Pricing {
  prompt_price: number;
  completion_price: number;
  cache_read_price: number;
  cache_write_price: number;
  image_input_price: number;
  audio_input_price: number;
  audio_output_price: number;
}

export interface ModelChannel {
  channel_id: string;
  priority: number;
  provider?: string;
  upstream_model?: string | null;
  max_tokens?: number | null;
}

export interface MarketplaceFormats {
  openai: boolean;
  anthropic: boolean;
}

export interface MarketplaceModel {
  name: string;
  pricing: Pricing;
  context_length?: number | null;
  category?: string;
  formats: MarketplaceFormats;
}

export interface Model {
  id: string;
  name: string;
  model_pattern: string;
  pricing: Pricing;
  channels: ModelChannel[];
  published?: boolean;
  context_length?: number | null;
  category?: string;
}

export interface RoutingRule {
  id: string;
  name: string;
  scope: string;
  user_id: string;
  source_model: string;
  target_model: string;
  channel_id: string;
  upstream_model: string;
  priority: number;
  enabled: boolean;
  description: string;
  created_at: string;
  updated_at: string;
  /** Team scope. Present when the rule applies to a team. */
  team_id?: string | null;
}

export interface UsageRecord {
  timestamp: string;
  request_id: string;
  user_id: string;
  user_name: string;
  channel_id: string;
  model: string;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  latency_ms: number;
  status_code: number;
  success: boolean;
  request_body?: string | null;
  response_body?: string | null;
  reasoning_body?: string | null;
  api_key_name?: string | null;
  api_format?: string;
  stream: boolean;
  cache_hit_input_tokens: number;
  cache_write_tokens: number;
  prompt_price: number;
  completion_price: number;
  cache_read_price?: number;
  cache_write_price?: number;
  original_model?: string;
  client_ip?: string | null;
  /** Team scope. Null means the request used a personal API key. */
  team_id?: string | null;
  billing_payment_mode?: 'metered' | 'prepaid' | null;
  /** Endpoint that served the request. Null when no endpoint matched. */
  endpoint_id?: number | null;
  /** Time to first upstream response data for streaming requests. */
  ttft_ms?: number | null;
}

export interface DashboardStats {
  users: number;
  channels: number;
  models: number;
  rules: number;
  api_keys: number;
  endpoints: number;
  total_requests: number;
}

export interface TopModel {
  model: string;
  count: number;
  percentage: number;
}

export interface DashboardAggregations {
  total_requests: number;
  total_cost: number;
  requests_24h: number;
  cost_24h: number;
  success_rate_24h: number;
  avg_latency_ms_24h: number;
  total_tokens_24h: number;
  top_models_24h: TopModel[];
}

export interface DailyUsage {
  date: string;
  count: number;
}

export interface ModelActivity {
  model: string;
  total_requests: number;
  prompt_tokens: number;
  completion_tokens: number;
  cache_hit_tokens: number;
  success_count: number;
  failure_count: number;
}

export interface DailyAggregate {
  date: string;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  success_count: number;
  latency_ms: number;
  cache_hit_tokens: number;
}

export interface FlowMetricsPercentiles {
  p50: number | null;
  p90: number | null;
  p99: number | null;
  sample_count: number;
}

export interface FlowMetricsModelShare {
  model: string;
  requests: number;
  share: number;
}

export interface FlowMetricsClientIp {
  ip: string;
  requests: number;
}

export interface FlowMetricsTrend {
  bucket_unit: 'minute' | 'hour';
  buckets: string[];
  success_completed: number[];
  failed_completed: number[];
}

export interface FlowMetricsHistorical {
  total_completed: number;
  success_completed: number;
  failed_completed: number;
  model_share: FlowMetricsModelShare[];
  client_ips: FlowMetricsClientIp[];
  latency_ms: FlowMetricsPercentiles;
  ttft_ms: FlowMetricsPercentiles;
  trend: FlowMetricsTrend;
}

export interface FlowMetricsRealtimeQueue {
  status: string;
  count: number | null;
  reason: string;
}

export interface FlowMetricsRealtime {
  as_of: string;
  in_flight: number;
  upstream_generating: number;
  upstream_outputting: number;
  queue: FlowMetricsRealtimeQueue;
  consistency: string;
  source: string;
}

export interface FlowMetricsResponse {
  schema_version: number;
  range: {
    start: string;
    end: string;
    model?: string | null;
  };
  historical: FlowMetricsHistorical;
  realtime: FlowMetricsRealtime;
}

export interface LoginResponse {
  token?: string;
  role: UserRole;
  user_id: string;
  user_name: string;
  timezone?: string;
  currency?: string;
}

export interface CurrentSessionResponse {
  user_id: string;
  user_name: string;
  role: UserRole;
  status: UserStatus;
  timezone: string;
  currency: string;
}

export interface CreateUserReq {
  id: string;
  name: string;
  password?: string | null;
  rate_limits?: RateLimit | null;
  role?: string | null;
  concurrency_limit?: number;
}

export interface UpdateUserReq {
  name?: string | null;
  password?: string | null;
  rate_limits?: RateLimit | null;
  role?: string | null;
  concurrency_limit?: number;
}

export interface CreateKeyReq {
  name?: string | null;
  enabled?: boolean | null;
  expires_at?: string | null;
  spend_limit?: number | null;
  allowed_models?: string[] | null;
  /** 访问范围 = 资源类型（model / skill / mcp / gateway）。 */
  scopes?: string[] | null;
  billing_group_id?: string | null;
}

export type CreateMyKeyReq = CreateKeyReq;

export interface UpstreamModel {
  id: string;
  max_model_len?: number | null;
}

export interface GatewayRuntimeConfig {
  connect_timeout_secs: number;
  unary_base_timeout_secs: number;
  body_size_extra_secs_per_100kb: number;
  stream_first_byte_timeout_secs: number;
  stream_idle_timeout_secs: number;
  stream_total_timeout_secs: number;
  max_retries: number;
  handler_timeout_secs: number;
  cache_ttl_secs: number;
  billing_enabled: boolean;
}

export interface ChannelCheckResult {
  channel_id: string;
  channel_name: string;
  provider: string;
  endpoint_url: string;
  success: boolean;
  latency_ms: number;
  error?: string | null;
}

export interface ProbeResult {
  id: string;
  channel_id: string;
  model_id: string;
  success: boolean;
  latency_ms: number;
  error?: string | null;
  probed_at: string;
  /** Endpoint primary key in the channel config table. NULL for legacy rows. */
  endpoint_id?: number | null;
  /** The upstream model name used when probing (from the binding). */
  upstream_model?: string | null;
  endpoint_url?: string | null;
}

export interface ModelHealthCheckResult {
  model_id: string;
  channel_results: ProbeResult[];
}

export interface BillingSummary {
  total_requests: number;
  total_cost: number;
  balance: number;
}

export interface AdminBillingSummary {
  total_requests: number;
  total_tokens: number;
  total_cost: number;
}

export interface AdminBillingActivity {
  year: number;
  month: number;
  active_teams: number;
  active_users: number;
}

export interface AdminBillingTeamSpendRankItem {
  team_id: string;
  team_name: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  active_users: number;
}

export interface AdminBillingTeamSpendRankingResponse {
  items: AdminBillingTeamSpendRankItem[];
}

export interface AdminBillingTeamRow {
  team_id: string;
  team_name: string;
  owner_id: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  active_users: number;
  last_billed_at?: string | null;
}

export interface AdminBillingTeamsResponse {
  items: AdminBillingTeamRow[];
  total: number;
}

export interface AdminBillingTeamUserRow {
  user_id: string;
  user_name: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  last_billed_at?: string | null;
}

export interface AdminBillingTeamUsersResponse {
  team: {
    team_id: string;
    team_name: string;
  };
  year: number;
  month: number;
  items: AdminBillingTeamUserRow[];
  total: number;
}

export interface AdminBillingApiKeyActivityRow {
  api_key_name?: string | null;
  total_requests: number;
  total_tokens: number;
  last_request_at?: string | null;
}

export interface AdminBillingApiKeyActivityResponse {
  team: {
    team_id: string;
    team_name: string;
  };
  user_id: string;
  year: number;
  month: number;
  stable_key_identity: boolean;
  grouping_field: string;
  items: AdminBillingApiKeyActivityRow[];
  total: number;
}

export interface AdminBillingTrendPoint {
  date: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
}

export interface AdminBillingUserSpendRow {
  team_id?: string | null;
  team_name?: string | null;
  team_count: number;
  multi_team: boolean;
  user_id: string;
  user_name: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  api_key_count: number;
  last_billed_at?: string | null;
}

export interface AdminBillingUserSpendRankingResponse {
  items: AdminBillingUserSpendRow[];
}

export interface AdminBillingUserApiKeyCostRow {
  api_key_name?: string | null;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  prompt_tokens: number;
  completion_tokens: number;
  cache_hit_input_tokens: number;
  primary_model?: string | null;
  last_request_at?: string | null;
  team_id?: string | null;
  api_key_enabled?: boolean | null;
  api_key?: string | null;
}

export interface AdminBillingUserApiKeyCostResponse {
  team?: {
    team_id: string;
    team_name: string;
  } | null;
  user_id: string;
  user_name?: string | null;
  year: number;
  month: number;
  stable_key_identity: boolean;
  grouping_field: string;
  items: AdminBillingUserApiKeyCostRow[];
  total: number;
}

export interface AdminBillingApiKeyDetailModelRow {
  model: string;
  total_requests: number;
  total_tokens: number;
}

export interface AdminBillingApiKeyDetailChannelRow {
  channel_id: string;
  total_requests: number;
}

export interface AdminBillingApiKeyDetailResponse {
  team?: {
    team_id: string;
    team_name: string;
  } | null;
  user_id: string;
  api_key_name: string;
  year: number;
  month: number;
  stable_key_identity: boolean;
  grouping_field: string;
  total_requests: number;
  total_tokens: number;
  top_models: AdminBillingApiKeyDetailModelRow[];
  top_channels: AdminBillingApiKeyDetailChannelRow[];
  recent_requests: UsageRecord[];
}

export interface ContentFilterRule {
  id: string;
  name: string;
  pattern_type: 'regex' | 'keyword';
  pattern: string;
  action: 'block' | 'mask';
  scope: 'request' | 'response' | 'both';
  channel_id?: string | null;
  replacement?: string;
  enabled: boolean;
  priority: number;
  created_at: string;
  updated_at: string;
}

export interface SsoConfig {
  id: string;
  team_id?: string | null;
  provider_name: string;
  issuer_url: string;
  client_id: string;
  redirect_url: string;
  enabled: boolean;
  auto_create_user: boolean;
  domain_restrictions?: string | null;
  default_role: string;
  created_at: string;
  updated_at: string;
}

export interface SsoConfigRequest {
  team_id?: string | null;
  provider_name: string;
  issuer_url: string;
  client_id: string;
  client_secret: string;
  redirect_url: string;
  enabled: boolean;
  auto_create_user: boolean;
  domain_restrictions?: string | null;
  default_role: string;
}
