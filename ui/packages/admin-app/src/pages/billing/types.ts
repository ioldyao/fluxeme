export type BillingView =
  | 'overview'
  | 'users'
  | 'user-detail'
  | 'user-team'
  | 'teams'
  | 'team-detail'
  | 'keys'
  | 'key-detail'
  | 'requests'
  | 'request-detail';

export type BillingDetailTab = 'trend' | 'model' | 'token';

export interface BillingCopy {
  title: string;
  subtitle: string;
  exportLabel: string;
  channelCostLabel: string;
  monthlyReportLabel: string;
  filterLabel: string;
  monthLabel: string;
  modelLabel: string;
  channelLabel: string;
  allTeams: string;
  allUsers: string;
  allModels: string;
  allChannels: string;
  searchPlaceholder: string;
  totalCost: string;
  teamsMetric: string;
  usersMetric: string;
  apiKeysMetric: string;
  activeUsers: string;
  members: string;
  apiKeys: string;
  scopeLabel: string;
  scopeGlobal: string;
  scopeTeam: string;
  scopeUser: string;
  scopeTrailHint: string;
  teamTableTitle: string;
  userTableTitle: string;
  apiKeyTableTitle: string;
  latestRequest: string;
  primaryModel: string;
  amount: string;
  totalRequests: string;
  totalTokens: string;
  tokens: string;
  action: string;
  openTeam: string;
  openUser: string;
  openApiKey: string;
  noMonthData: string;
  noData: string;
  noTeams: string;
  noUsers: string;
  noApiKeys: string;
  noRequests: string;
  noSelectionTeam: string;
  noSelectionUser: string;
  currentScopeOverview: string;
  avgUnitCost: string;
  topModelShare: string;
  cacheHitShare: string;
  channelShare: string;
  supplementTitle: string;
  supplementSub: string;
  tabTrend: string;
  tabModelCost: string;
  tabTokenCost: string;
  noTrendData: string;
  noModelBreakdown: string;
  noTokenBreakdown: string;
  drawerTitle: string;
  drawerHint: string;
  topModels: string;
  topChannels: string;
  recentRequests: string;
  requestDetailTitle: string;
  requestUnit: string;
  viewRequest: string;
  teamColumnHint: string;
  userColumnHint: string;
  apiKeyColumnHint: string;
  groupingHint: string;
  requestLabel: string;
  responseLabel: string;
  reasoningLabel: string;
  promptLabel: string;
  completionLabel: string;
  cacheLabel: string;
  teamLabel: string;
  userLabel: string;
}

export interface BillingMonthOption {
  raw: string;
  label: string;
  year: number;
  month: number;
}

export interface BillingMetricCard {
  label: string;
  value: string;
  meta?: string;
}

export interface BillingUserRow {
  user_id: string;
  user_name: string;
  team_id?: string | null;
  team_name?: string | null;
  team_count?: number;
  multi_team?: boolean;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
  api_key_count?: number;
  last_billed_at?: string | null;
}
