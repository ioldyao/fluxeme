import { useState, useMemo, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { formatTimestamp } from '@fluxeme/shared/src/lib/date';
import { useUsageRequests, useAdminUsageBilling, useUsageAnalytics, useRecentClientIps } from '@fluxeme/shared/src/api/usage';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { useCurrency } from '@fluxeme/shared/src/store/currency';
import { UsageAnalyticsCharts } from '@fluxeme/shared/src/components/usage/UsageAnalyticsCharts';
import { Combobox } from '@fluxeme/shared/src/components/ui/Combobox';
import { UsageLogDetail } from '../components/UsageLogDetail';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { DateRangePicker } from '@fluxeme/shared/src/components/ui/date-range-picker';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@fluxeme/shared/src/components/ui/tabs';
import { RefreshCw, Filter, ChevronDown, ChevronRight, List, BarChart3, Radio, RadioIcon } from 'lucide-react';

type RequestStatus = 'succeeded' | 'rejected' | 'failed' | 'cancelled';

const STATUS_BADGE: Record<RequestStatus, string> = {
  succeeded: 'bg-chart-2/10 text-chart-2',
  rejected: 'bg-amber-100 text-amber-700',
  failed: 'bg-destructive/10 text-destructive',
  cancelled: 'bg-muted text-muted-foreground',
};

function statusBadgeClass(status: string): string {
  return STATUS_BADGE[status as RequestStatus] ?? 'bg-secondary text-foreground';
}

const ERROR_KIND_LABEL: Record<string, string> = {
  model_not_found: 'usage.errModelNotFound',
  no_available_endpoint: 'usage.errNoEndpoint',
  rate_limit_exceeded: 'usage.errRateLimit',
  stream_idle_timeout: 'usage.errStreamIdle',
  auth_failed: 'usage.errAuth',
  upstream_error: 'usage.errUpstream',
  upstream_timeout: 'usage.errUpstreamTimeout',
  bad_request: 'usage.errBadRequest',
};

function errorKindLabel(kind: string | null | undefined, t: (k: string) => string): string {
  if (!kind) return '—';
  const key = ERROR_KIND_LABEL[kind];
  return key ? t(key) : kind;
}

export default function UsageLog() {
  const { t } = useTranslation();
  const [limit, setLimit] = useState(20);
  const [offset, setOffset] = useState(0);
  const [showFilters, setShowFilters] = useState(false);
  const [userIdFilter, setUserIdFilter] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [apiKeyFilter, setApiKeyFilter] = useState('');
  const [apiFormatFilter, setApiFormatFilter] = useState('');
  const [requestIdFilter, setRequestIdFilter] = useState('');
  const [channelNameFilter, setChannelNameFilter] = useState('');
  const [channelIdFilter, setChannelIdFilter] = useState('');
  const [endpointFilter, setEndpointFilter] = useState('');
  const [clientIpFilter, setClientIpFilter] = useState('');
  const [startDt, setStartDt] = useState('');
  const [endDt, setEndDt] = useState('');
  const [detailId, setDetailId] = useState<string | null>(null);
  const [chartTab, setChartTab] = useState('list');
  const [chartDateFilter, setChartDateFilter] = useState('7d');
  const [chartStartDt, setChartStartDt] = useState('');
  const [chartEndDt, setChartEndDt] = useState('');

  // ── Date filter (supports ?date=YYYY-MM-DD from wallet navigation) ──
  const [searchParams] = useSearchParams();
  const urlDate = searchParams.get('date');
  const [dateFilter, setDateFilter] = useState(urlDate || 'all');
  useEffect(() => {
    if (urlDate && urlDate !== dateFilter) {
      setDateFilter(urlDate);
    }
  }, [urlDate, dateFilter]);
  const dateParams = useMemo(() => {
    // Custom datetime range takes priority over quick tabs when set.
    if (startDt || endDt) {
      return {
        ...(startDt ? { start_date: new Date(startDt).toISOString() } : {}),
        ...(endDt ? { end_date: new Date(endDt).toISOString() } : {}),
      };
    }
    if (dateFilter === 'all') return {};
    if (dateFilter === 'today') {
      const now = new Date();
      const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      return { start_date: start.toISOString(), end_date: now.toISOString() };
    }
    if (dateFilter === '7d') {
      const start = new Date(Date.now() - 7 * 86400000);
      return { start_date: start.toISOString() };
    }
    if (dateFilter === '30d') {
      const start = new Date(Date.now() - 30 * 86400000);
      return { start_date: start.toISOString() };
    }
    const startLocal = new Date(`${dateFilter}T00:00:00`);
    const endLocal = new Date(`${dateFilter}T23:59:59`);
    return { start_date: startLocal.toISOString(), end_date: endLocal.toISOString() };
  }, [dateFilter, startDt, endDt]);
  const isCustomDate = dateFilter.length === 10 && dateFilter.includes('-');

  const filtersActive = !!userIdFilter || !!modelFilter || !!apiKeyFilter || !!apiFormatFilter
    || !!requestIdFilter || !!channelNameFilter || !!channelIdFilter || !!endpointFilter || !!clientIpFilter
    || dateFilter !== 'all' || !!startDt || !!endDt;
  const params = {
    limit, offset,
    ...(userIdFilter ? { user_id: userIdFilter } : {}),
    ...(modelFilter ? { model: modelFilter } : {}),
    ...(apiKeyFilter ? { api_key: apiKeyFilter } : {}),
    ...(apiFormatFilter ? { api_format: apiFormatFilter } : {}),
    ...(requestIdFilter ? { request_id: requestIdFilter } : {}),
    ...(channelNameFilter ? { channel_name: channelNameFilter } : {}),
    ...(channelIdFilter ? { channel_id: channelIdFilter } : {}),
    ...(endpointFilter ? { endpoint_url: endpointFilter } : {}),
    ...(clientIpFilter ? { client_ip: clientIpFilter } : {}),
    ...dateParams,
  };
  const { data: usage, isLoading, isError, refetch } = useUsageRequests(params);
  const records = usage?.records ?? [];
  const requestIds = useMemo(() => records.map((record) => record.request_id), [records]);
  const { data: billingRows, isError: isBillingError } = useAdminUsageBilling(requestIds);
  const billingByRequestId = useMemo(
    () => new Map((billingRows ?? []).map((row) => [row.request_id, row])),
    [billingRows],
  );
  // Channel id → display name map, reused by the "Channel Name ID" column.
  // Channel list is react-query cached, so no per-row fetch is needed.
  const { data: channelsList } = useChannels();
  const channelNameById = useMemo(() => {
    const map = new Map<string, string>();
    for (const ch of channelsList ?? []) {
      map.set(ch.id, ch.name);
    }
    return map;
  }, [channelsList]);

  // Combobox suggestion sources — full catalogs from the cached channel list
  // plus values already visible on the current page / recent client IPs.
  const channelNameOptions = useMemo(() => [...new Set((channelsList ?? []).map((c) => c.name).filter(Boolean))], [channelsList]);
  const channelIdOptions = useMemo(() => [...new Set((channelsList ?? []).map((c) => c.id).filter(Boolean))], [channelsList]);
  const endpointOptions = useMemo(
    () => [...new Set((channelsList ?? []).flatMap((c) => c.endpoints ?? []).map((e) => e.url).filter(Boolean))],
    [channelsList],
  );
  const requestIdOptions = useMemo(() => [...new Set(records.map((r) => r.request_id))], [records]);
  const { data: recentIps } = useRecentClientIps();
  const clientIpOptions = useMemo(() => [...new Set((recentIps ?? []).filter(Boolean))], [recentIps]);

  const { currency } = useCurrency();
  const total = usage?.total ?? 0;
  const page = offset / limit + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  // Chart time window — mirrors the list filter's date widget (quick tabs +
  // DateRangePicker). Unlike the paginated list, a chart always needs a bounded
  // window, so "all" falls back to the last 30 days.
  const chartDateParams = useMemo(() => {
    if (chartStartDt || chartEndDt) {
      return {
        ...(chartStartDt ? { start_date: new Date(chartStartDt).toISOString() } : {}),
        ...(chartEndDt ? { end_date: new Date(chartEndDt).toISOString() } : {}),
      };
    }
    if (chartDateFilter === 'today') {
      const now = new Date();
      const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      return { start_date: start.toISOString(), end_date: now.toISOString() };
    }
    if (chartDateFilter === '7d') {
      return { start_date: new Date(Date.now() - 7 * 86400000).toISOString(), end_date: new Date().toISOString() };
    }
    if (chartDateFilter === '30d' || chartDateFilter === 'all') {
      return { start_date: new Date(Date.now() - 30 * 86400000).toISOString(), end_date: new Date().toISOString() };
    }
    return { days: 7 };
  }, [chartDateFilter, chartStartDt, chartEndDt]);

  const {
    data: analytics,
    isLoading: analyticsLoading,
    isFetching: analyticsFetching,
    isError: analyticsError,
    refetch: refetchAnalytics,
  } = useUsageAnalytics({ ...chartDateParams, enabled: chartTab === 'chart' });

  return (
    <div className="animate-fade-in">
      <PageHeader
        title={t('usage.title')}
        description={t('usage.adminSubtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => { void refetch(); void refetchAnalytics(); }}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      <Tabs value={chartTab} onValueChange={setChartTab}>
        <TabsList className="w-fit justify-start border-b bg-transparent p-0">
          <TabsTrigger className="flex-none px-3" value="list">
            <List className="size-4 mr-1" />{t('usage.list')}
          </TabsTrigger>
          <TabsTrigger className="flex-none px-3" value="chart">
            <BarChart3 className="size-4 mr-1" />{t('usage.chart')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="list" className="mt-5 space-y-4">
          {/* Collapsible filter bar */}
          <div className="mb-5 flex flex-wrap items-center gap-3">
            <Button variant="outline" size="sm" onClick={() => setShowFilters(!showFilters)}>
              <Filter className="size-4 mr-1" />
              {t('usage.filter')}
              {filtersActive && <span className="ml-1.5 size-2 rounded-full bg-primary" />}
              {showFilters ? <ChevronDown className="size-3 ml-1" /> : <ChevronRight className="size-3 ml-1" />}
            </Button>
            <div className="flex items-center gap-2 ml-auto">
              <span className="text-xs text-muted-foreground whitespace-nowrap">{t('common.pageSize')}</span>
              <Select value={String(limit)} onValueChange={(v) => { setLimit(Number(v)); setOffset(0); }}>
                <SelectTrigger className="w-20 h-9">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="20">20</SelectItem>
                  <SelectItem value="50">50</SelectItem>
                  <SelectItem value="100">100</SelectItem>
                  <SelectItem value="200">200</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Filter inputs */}
          {showFilters && (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2 p-3 rounded-lg border bg-muted/30">
              <Input
                placeholder={t('usage.filterUser')}
                value={userIdFilter} onChange={(e) => { setUserIdFilter(e.target.value); setOffset(0); }}
              />
              <Combobox
                options={requestIdOptions}
                placeholder={t('usage.filterRequestId')}
                value={requestIdFilter}
                onValueChange={(v) => { setRequestIdFilter(v); setOffset(0); }}
              />
              <Input
                placeholder={t('usage.filterModel')}
                value={modelFilter} onChange={(e) => { setModelFilter(e.target.value); setOffset(0); }}
              />
              <Input
                placeholder={t('usage.filterApiKey')}
                value={apiKeyFilter} onChange={(e) => { setApiKeyFilter(e.target.value); setOffset(0); }}
              />
              <Select value={apiFormatFilter} onValueChange={(v) => { setApiFormatFilter(v); setOffset(0); }}>
                <SelectTrigger className="h-9">
                  <SelectValue placeholder={t('usage.filterApiFormat')} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="">All</SelectItem>
                  <SelectItem value="openai">OpenAI</SelectItem>
                  <SelectItem value="anthropic">Anthropic</SelectItem>
                  <SelectItem value="relay">Relay</SelectItem>
                </SelectContent>
              </Select>
              <Combobox
                options={channelNameOptions}
                placeholder={t('usage.filterChannelName')}
                value={channelNameFilter}
                onValueChange={(v) => { setChannelNameFilter(v); setOffset(0); }}
              />
              <Combobox
                options={channelIdOptions}
                placeholder={t('usage.filterChannelId')}
                value={channelIdFilter}
                onValueChange={(v) => { setChannelIdFilter(v); setOffset(0); }}
              />
              <Combobox
                options={endpointOptions}
                placeholder={t('usage.filterEndpoint')}
                value={endpointFilter}
                onValueChange={(v) => { setEndpointFilter(v); setOffset(0); }}
              />
              <Combobox
                options={clientIpOptions}
                placeholder={t('usage.filterClientIp')}
                value={clientIpFilter}
                onValueChange={(v) => { setClientIpFilter(v); setOffset(0); }}
              />
            </div>
          )}

          {/* Date range filter tabs + custom datetime range */}
          {showFilters && (
            <div className="mt-3 mb-5 flex flex-wrap items-center gap-2 text-xs">
              <div className="flex items-center gap-1">
                {(['today', '7d', '30d', 'all'] as const).map((key) => (
                  <button
                    key={key}
                    onClick={() => { setDateFilter(key); setStartDt(''); setEndDt(''); setOffset(0); }}
                    className={`px-2.5 py-1 rounded-md font-medium transition-colors ${
                      (!isCustomDate && !startDt && !endDt && dateFilter === key)
                        ? 'bg-brand text-white'
                        : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                    }`}
                  >
                    {key === 'today' ? t('usage.dateToday') : key === '7d' ? t('usage.date7d') : key === '30d' ? t('usage.date30d') : t('usage.dateAll')}
                  </button>
                ))}
                {isCustomDate && (
                  <span className="px-2.5 py-1 rounded-md bg-brand text-white font-medium">
                    {dateFilter}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-1.5 ml-auto">
                <DateRangePicker
                  start={startDt}
                  end={endDt}
                  onStartChange={(v) => { setStartDt(v); setOffset(0); }}
                  onEndChange={(v) => { setEndDt(v); setOffset(0); }}
                  startPlaceholder={t('usage.startTime')}
                  endPlaceholder={t('usage.endTime')}
                  className="w-auto"
                />
              </div>
            </div>
          )}

          <Card>
            <CardContent className="p-0">
              {isLoading ? (
                <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
              ) : isError ? (
                <div className="flex items-center justify-center p-8">
                  <div className="text-center">
                    <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
                    <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
                  </div>
                </div>
              ) : records.length > 0 ? (
                <div className="overflow-x-auto rounded-xl border border-border">
                  <table className="w-full min-w-[1920px] table-fixed border-collapse text-sm">
                    <colgroup>
                      <col className="w-[130px]" /><col className="w-[90px]" /><col className="w-[160px]" /><col className="w-[160px]" />
                      <col className="w-[110px]" /><col className="w-[230px]" /><col className="w-[90px]" />
                      <col className="w-[120px]" /><col className="w-[100px]" /><col className="w-[90px]" />
                      <col className="w-[100px]" /><col className="w-[100px]" />
                      <col className="w-[90px]" /><col className="w-[100px]" /><col className="w-[130px]" />
                      <col className="w-[140px]" /><col className="w-[110px]" /><col className="w-[110px]" />
                      <col className="w-[150px]" /><col className="w-[90px]" />
                    </colgroup>
                    <thead>
                      <tr className="border-b bg-muted/30 text-[11px] font-semibold text-muted-foreground">
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.time')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-center">{t('table.status')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.user')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.apiKey')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.billingMode')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.model')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.apiFormat')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.channelName')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.channelId')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.endpointId')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.uncachedInput')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.cachedInput')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('dash.completion')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.totalTokens')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">资源包 units</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">钱包实扣</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('table.latency')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.clientIp')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.errorKind')}</th>
                        <th className="whitespace-nowrap px-3 py-3 text-right">{t('usage.attemptCount')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {records.map((r) => (
                        <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50 cursor-pointer" onClick={() => setDetailId(r.request_id)}>
                          <td className="whitespace-nowrap px-3 py-3 text-xs text-muted-foreground">
                            {formatTimestamp(r.timestamp)}
                          </td>
                          <td className="whitespace-nowrap px-3 py-3 text-center" aria-label={r.status}>
                            <span className={`inline-flex rounded-full px-2 py-0.5 text-[10px] font-medium ${statusBadgeClass(r.status)}`}>{r.status}</span>
                          </td>
                          <td className="max-w-[160px] truncate whitespace-nowrap px-3 py-3" title={r.user_name ?? undefined}>{r.user_name}</td>
                          <td className="max-w-[160px] truncate whitespace-nowrap px-3 py-3" title={r.api_key_name ?? undefined}>{r.api_key_name ?? '—'}</td>
                          <td className="whitespace-nowrap px-3 py-3"><span className={`inline-flex rounded-full px-2 py-0.5 text-[10px] font-medium ${r.billing_payment_mode === 'prepaid' ? 'bg-amber-100 text-amber-700' : 'bg-blue-100 text-blue-700'}`}>{r.billing_payment_mode === 'prepaid' ? t('usage.prepaid') : t('usage.metered')}</span></td>
                          <td className="max-w-[230px] whitespace-nowrap px-3 py-3">
                            <span className="inline-flex max-w-full min-w-0 items-center gap-1">
                              <span className="min-w-0 truncate" title={(r.resolved_model || r.requested_model)}>{(r.resolved_model || r.requested_model)}</span>
                              {r.stream ? (
                                <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-accent-foreground bg-accent border border-border px-1.5 py-0.5 rounded">
                                  <Radio className="h-2.5 w-2.5" />stream
                                </span>
                              ) : (
                                <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-muted-foreground bg-secondary border border-border px-1.5 py-0.5 rounded">
                                  <RadioIcon className="h-2.5 w-2.5" />sync
                                </span>
                              )}
                            </span>
                          </td>
                          <td className="whitespace-nowrap px-3 py-3 font-mono text-xs">{r.api_format ?? '—'}</td>
                          <td className="max-w-[120px] truncate whitespace-nowrap px-3 py-3" title={channelNameById.get(r.channel_id ?? '') ?? undefined}>
                            {channelNameById.get(r.channel_id ?? '') ?? '—'}
                          </td>
                          <td className="whitespace-nowrap px-3 py-3 font-mono text-xs">{r.channel_id ?? '—'}</td>
                          <td className="whitespace-nowrap px-3 py-3 font-mono text-xs">{r.endpoint_id ?? '—'}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{r.prompt_tokens.toLocaleString()}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right text-muted-foreground tabular-nums">{(r.cache_read_tokens ?? r.cache_hit_input_tokens ?? 0) > 0 ? (r.cache_read_tokens ?? r.cache_hit_input_tokens ?? 0).toLocaleString() : '—'}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{r.completion_tokens.toLocaleString()}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right font-medium tabular-nums">{(r.prompt_tokens + (r.cache_read_tokens ?? r.cache_hit_input_tokens ?? 0) + r.completion_tokens).toLocaleString()}</td>
                          {(() => {
                            const billing = billingByRequestId.get(r.request_id);
                            const wallet = currency === 'cny' ? '¥' : '$';
                            const walletAmount = Number(billing?.wallet_amount) || 0;
                            return <>
                              <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{billing?.package_units?.toLocaleString() ?? '—'}</td>
                              <td className="whitespace-nowrap px-3 py-3 text-right font-mono text-xs tabular-nums">{isBillingError || !billing || billing.wallet_debit_status === 'unavailable' ? '—' : billing.wallet_debit_status === 'pending' ? t('usage.settlementPending') : `${wallet}${walletAmount.toFixed(6)}`}</td>
                            </>;
                          })()}
                          <td className="whitespace-nowrap px-3 py-3 text-right text-muted-foreground tabular-nums">{r.total_latency_ms.toLocaleString()}ms</td>
                          <td className="max-w-[110px] truncate whitespace-nowrap px-3 py-3 text-xs font-mono text-muted-foreground" title={r.client_ip ?? undefined}>{r.client_ip ?? '—'}</td>
                          <td className="max-w-[150px] truncate px-3 py-3 text-xs" title={r.error_kind ?? undefined}>{errorKindLabel(r.error_kind, t)}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{r.attempt_count}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <EmptyState message={t('empty.noUsage')} />
              )}
              {records.length > 0 && (
                <div className="flex flex-wrap items-center justify-between gap-3 border-t px-4 py-3">
                  <span className="text-xs text-muted-foreground">
                    {total > 0 && `${(page - 1) * limit + 1}–${Math.min(page * limit, total)} / ${total}`}
                  </span>
                  <div className="flex flex-wrap items-center justify-end gap-1">
                    <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setOffset(0)} title="第一页">
                      ⟪
                    </Button>
                    <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setOffset(offset - limit)}>
                      {t('common.prev')}
                    </Button>
                    {Array.from({ length: Math.min(totalPages, 5) }, (_, i) => {
                      const start = Math.max(0, Math.min(page - 3, totalPages - 5));
                      const p = start + i + 1;
                      return (
                        <Button key={p} variant={p === page ? 'default' : 'outline'} size="sm" className="w-8" onClick={() => setOffset((p - 1) * limit)}>
                          {p}
                        </Button>
                      );
                    })}
                    <Button variant="outline" size="sm" disabled={page >= totalPages} onClick={() => setOffset(offset + limit)}>
                      {t('common.next')}
                    </Button>
                    <Button variant="outline" size="sm" disabled={page >= totalPages} onClick={() => setOffset((totalPages - 1) * limit)} title="最后一页">
                      ⟫
                    </Button>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="chart" className="space-y-4">
          {/* Chart time window — same widget as the list filter */}
          <div className="flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card p-3 text-xs">
            <div className="flex flex-wrap items-center gap-1">
              {(['today', '7d', '30d', 'all'] as const).map((key) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => { setChartDateFilter(key); setChartStartDt(''); setChartEndDt(''); }}
                  className={`px-2.5 py-1 rounded-md font-medium transition-colors ${
                    (!chartStartDt && !chartEndDt && chartDateFilter === key)
                      ? 'bg-brand text-white'
                      : 'text-muted-foreground hover:text-foreground hover:bg-accent'
                  }`}
                >
                  {key === 'today' ? t('usage.dateToday') : key === '7d' ? t('usage.date7d') : key === '30d' ? t('usage.date30d') : t('usage.dateAll')}
                </button>
              ))}
            </div>
            <div className="ml-auto flex w-full items-center gap-1.5 sm:w-auto">
              <DateRangePicker
                start={chartStartDt}
                end={chartEndDt}
                onStartChange={(v) => { setChartStartDt(v); }}
                onEndChange={(v) => { setChartEndDt(v); }}
                startPlaceholder={t('usage.startTime')}
                endPlaceholder={t('usage.endTime')}
                className="w-full sm:w-auto"
              />
            </div>
          </div>

          <UsageAnalyticsCharts
            data={analytics}
            isLoading={analyticsLoading}
            isFetching={analyticsFetching}
            isError={analyticsError}
            onRetry={() => { void refetchAnalytics(); }}
          />
        </TabsContent>
      </Tabs>

      <UsageLogDetail
        requestId={detailId}
        open={!!detailId}
        onOpenChange={(open) => { if (!open) setDetailId(null); }}
      />
    </div>
  );
}
