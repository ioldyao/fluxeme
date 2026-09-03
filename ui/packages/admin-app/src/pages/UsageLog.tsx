import { useState, useMemo, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { formatTimestamp } from '@fluxeme/shared/src/lib/date';
import { useUsage, useAdminUsageBilling } from '@fluxeme/shared/src/api/usage';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { useCurrency } from '@fluxeme/shared/src/store/currency';
import { UsageLogDetail } from '../components/UsageLogDetail';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { DateRangePicker } from '@fluxeme/shared/src/components/ui/date-range-picker';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { RefreshCw, CheckCircle2, XCircle, Filter, ChevronDown, ChevronRight } from 'lucide-react';

export default function UsageLog() {
  const { t } = useTranslation();
  const [limit, setLimit] = useState(20);
  const [offset, setOffset] = useState(0);
  const [showFilters, setShowFilters] = useState(false);
  const [userIdFilter, setUserIdFilter] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [apiKeyFilter, setApiKeyFilter] = useState('');
  const [apiFormatFilter, setApiFormatFilter] = useState('');
  const [startDt, setStartDt] = useState('');
  const [endDt, setEndDt] = useState('');
  const [detailId, setDetailId] = useState<string | null>(null);

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

  const filtersActive = !!userIdFilter || !!modelFilter || !!apiKeyFilter || !!apiFormatFilter || dateFilter !== 'all' || !!startDt || !!endDt;
  const params = {
    limit, offset,
    ...(userIdFilter ? { user_id: userIdFilter } : {}),
    ...(modelFilter ? { model: modelFilter } : {}),
    ...(apiKeyFilter ? { api_key: apiKeyFilter } : {}),
    ...(apiFormatFilter ? { api_format: apiFormatFilter } : {}),
    ...dateParams,
  };
  const { data: usage, isLoading, isError, refetch } = useUsage(params);
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
  const channelLabel = (id: string) => {
    const name = channelNameById.get(id);
    return name ? `${name} ${id}` : id;
  };
  const { currency } = useCurrency();
  const total = usage?.total ?? 0;
  const page = offset / limit + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));

  return (
    <div className="animate-fade-in">
      <PageHeader
        title={t('usage.title')}
        description={t('usage.adminSubtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

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
              <table className="w-full min-w-[1500px] table-fixed border-collapse text-sm">
                <colgroup>
                  <col className="w-[130px]" /><col className="w-[160px]" /><col className="w-[160px]" />
                  <col className="w-[110px]" /><col className="w-[230px]" /><col className="w-[90px]" />
                  <col className="w-[200px]" /><col className="w-[100px]" /><col className="w-[100px]" />
                  <col className="w-[90px]" />
                  <col className="w-[100px]" /><col className="w-[130px]" /><col className="w-[140px]" />
                  <col className="w-[110px]" /><col className="w-[70px]" />
                </colgroup>
                <thead>
                  <tr className="border-b bg-muted/30 text-[11px] font-semibold text-muted-foreground">
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.time')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.user')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.apiKey')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.billingMode')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('table.model')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.apiFormat')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-left">{t('usage.channelNameId')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.uncachedInput')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.cachedInput')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('dash.completion')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('usage.totalTokens')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">资源包 units</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">钱包实扣</th>
                    <th className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{t('table.latency')}</th>
                    <th className="whitespace-nowrap px-3 py-3 text-center">{t('table.status')}</th>
                  </tr>
                </thead>
                <tbody>
                  {records.map((r) => (
                    <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50 cursor-pointer" onClick={() => setDetailId(r.request_id)}>
                      <td className="whitespace-nowrap px-3 py-3 text-xs text-muted-foreground">
                        {formatTimestamp(r.timestamp)}
                      </td>
                      <td className="max-w-[160px] truncate whitespace-nowrap px-3 py-3" title={r.user_name}>{r.user_name}</td>
                      <td className="max-w-[160px] truncate whitespace-nowrap px-3 py-3" title={r.api_key_name ?? undefined}>{r.api_key_name ?? '—'}</td>
                      <td className="whitespace-nowrap px-3 py-3"><span className={`inline-flex rounded-full px-2 py-0.5 text-[10px] font-medium ${r.billing_payment_mode === 'prepaid' ? 'bg-amber-100 text-amber-700' : 'bg-blue-100 text-blue-700'}`}>{r.billing_payment_mode === 'prepaid' ? t('usage.prepaid') : t('usage.metered')}</span></td>
                      <td className="max-w-[230px] truncate whitespace-nowrap px-3 py-3" title={r.model}>{r.model}</td>
                      <td className="whitespace-nowrap px-3 py-3 font-mono text-xs">{r.api_format ?? '—'}</td>
                      <td className="max-w-[200px] truncate whitespace-nowrap px-3 py-3" title={channelLabel(r.channel_id)}>{channelLabel(r.channel_id)}</td>
                      <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{r.prompt_tokens.toLocaleString()}</td>
                      <td className="whitespace-nowrap px-3 py-3 text-right text-muted-foreground tabular-nums">{r.cache_hit_input_tokens > 0 ? r.cache_hit_input_tokens.toLocaleString() : '—'}</td>
                      <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{r.completion_tokens.toLocaleString()}</td>
                      <td className="whitespace-nowrap px-3 py-3 text-right font-medium tabular-nums">{(r.prompt_tokens + r.cache_hit_input_tokens + r.completion_tokens).toLocaleString()}</td>
                      {(() => {
                        const billing = billingByRequestId.get(r.request_id);
                        const wallet = currency === 'cny' ? '¥' : '$';
                        const walletAmount = Number(billing?.wallet_amount) || 0;
                        return <>
                          <td className="whitespace-nowrap px-3 py-3 text-right tabular-nums">{billing?.package_units?.toLocaleString() ?? '—'}</td>
                          <td className="whitespace-nowrap px-3 py-3 text-right font-mono text-xs tabular-nums">{isBillingError || !billing || billing.wallet_debit_status === 'unavailable' ? '—' : billing.wallet_debit_status === 'pending' ? t('usage.settlementPending') : `${wallet}${walletAmount.toFixed(6)}`}</td>
                        </>;
                      })()}
                      <td className="whitespace-nowrap px-3 py-3 text-right text-muted-foreground tabular-nums">{r.latency_ms.toLocaleString()}ms</td>
                      <td className="whitespace-nowrap px-3 py-3 text-center" aria-label={r.success ? t('usage.success') : t('usage.failure')}>
                        {r.success ? (
                          <CheckCircle2 className="size-4 text-chart-2 inline" />
                        ) : (
                          <XCircle className="size-4 text-destructive inline" />
                        )}
                      </td>
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

      <UsageLogDetail
        requestId={detailId}
        open={!!detailId}
        onOpenChange={(open) => { if (!open) setDetailId(null); }}
      />
    </div>
  );
}
