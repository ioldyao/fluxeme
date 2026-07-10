export type UserRole = 'admin' | 'user';

export interface RateLimit {
  rpm: number | null;
  tpm: number | null;
}

export interface User {
  id: string;
  name: string;
  rate_limits?: RateLimit | null;
}

export interface UserDetail extends User {
  keys: ApiKey[];
}

export interface ApiKey {
  key: string;
  user_id: string;
  name: string;
  enabled: boolean;
  expires_at?: string | null;
  spend_limit?: number | null;
  allowed_models?: string[] | null;
}

export interface Endpoint {
  url: string;
  api_key: string;
  weight: number;
  timeout_secs?: number | null;
}

export type Provider = 'openai' | 'anthropic' | 'vllm' | 'azure' | 'ollama' | string;

export interface Channel {
  id: string;
  name: string;
  provider: Provider;
  priority: number;
  enabled: boolean;
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
}

export interface Model {
  id: string;
  name: string;
  model_pattern: string;
  pricing: Pricing;
  channels: ModelChannel[];
  published?: boolean;
  context_length?: number | null;
}

export interface RoutingRule {
  name: string;
  user_id: string;
  model_pattern: string;
  channel_id: string;
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

export interface LoginResponse {
  token: string;
  role: UserRole;
  user_id: string;
  user_name: string;
}

export interface CreateUserReq {
  id: string;
  name: string;
  password?: string | null;
  rate_limits?: RateLimit | null;
}

export interface UpdateUserReq {
  name?: string | null;
  password?: string | null;
  rate_limits?: RateLimit | null;
}

export interface CreateKeyReq {
  name?: string | null;
  enabled?: boolean | null;
  expires_at?: string | null;
  spend_limit?: number | null;
  allowed_models?: string[] | null;
}

export type CreateMyKeyReq = CreateKeyReq;

export interface UpstreamModel {
  id: string;
  max_model_len?: number | null;
}
