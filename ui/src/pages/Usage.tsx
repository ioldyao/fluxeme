import { useState, useMemo, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useCurrency } from '@/store/currency';
import { usePermission } from '@/permissions';
import { formatCost, getRecordPricing } from '@/lib/cost';
import { useUsage, useUsageAggregate, useModelActivity } from '@/api/usage';
import { api } from '@/api/client';
import { UsageLogDetail } from '@/components/UsageLogDetail';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Search, RefreshCw, CheckCircle2, XCircle, BarChart3, List, Radio, RadioIcon, Filter, ChevronDown, ChevronRight } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer,
  LineChart, Line, Legend,
} from 'recharts';

function ChartTooltip({ active, payload, label, formatter, showTotal }: any) {
  const { t } = useTranslation();
  if (!active || !payload?.length) return null;
  const total = showTotal ? payload.reduce((sum: number, e: any) => sum + (e.value ?? 0), 0) : null;
  return (
    <div className="rounded-lg border bg-popover px-3 py-2 text-xs shadow-md">
      {label && <p className="mb-1 font-medium text-popover-foreground">{label}</p>}
      {payload.map((entry: any, i: number) => {
        const formatted = formatter?.(entry.value, entry.name) ?? (
          typeof entry.value === 'number' ? entry.value.toLocaleString() : entry.value
        );
        return (
          <div key={i} className="flex items-center gap-2 text-muted-foreground">
            <span className="size-2 rounded-full" style={{ background: entry.color }} />
            <span>{entry.name}</span>
            <span className="ml-auto font-mono font-medium text-popover-foreground">{formatted}</span>
          </div>
        );
      })}
      {total !== null && (
        <div className="mt-1.5 pt-1.5 border-t flex items-center gap-2 text-muted-foreground">
          <span className="font-medium">{t('dash.total')}</span>
          <span className="ml-auto font-mono font-medium text-popover-foreground">{total.toLocaleString()}</span>
        </div>
      )}
    </div>
  );
}

export default function Usage() {
  const { t } = useTranslation();
  const { role } = useAuth();
  const canFilterUsers = usePermission('admin:usage-filters');
  const [limit, setLimit] = useState(20);
  const [offset, setOffset] = useState(0);
  const [showFilters, setShowFilters] = useState(false);
  const [userFilter, setUserFilter] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [apiKeyFilter, setApiKeyFilter] = useState('');
  const [apiFormatFilter, setApiFormatFilter] = useState('');
  const [detailId, setDetailId] = useState<string | null>(null);

  // ── Date filter (supports ?date=YYYY-MM-DD from wallet navigation) ──
  const [searchParams] = useSearchParams();
  const urlDate = searchParams.get('date');
  const [dateFilter, setDateFilter] = useState(urlDate || 'all');
  useEffect(() => {
    if (urlDate && urlDate !== dateFilter) {
      setDateFilter(urlDate);
    }
  }, [urlDate]); // eslint-disable-line react-hooks/exhaustive-deps
  const dateParams = useMemo(() => {
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
    // Custom date from URL: dateFilter is YYYY-MM-DD — convert local date to UTC range
    const startLocal = new Date(`${dateFilter}T00:00:00`);
    const endLocal = new Date(`${dateFilter}T23:59:59`);
    return { start_date: startLocal.toISOString(), end_date: endLocal.toISOString() };
  }, [dateFilter]);
  const isCustomDate = dateFilter.length === 10 && dateFilter.includes('-');

  const filtersActive = !!(canFilterUsers && userFilter) || modelFilter || apiKeyFilter || apiFormatFilter || dateFilter !== 'all';
  const params = {
    limit, offset,
    ...(role === 'admin' && userFilter ? { user_id: userFilter } : {}),
    ...(modelFilter ? { model: modelFilter } : {}),
    ...(apiKeyFilter ? { api_key: apiKeyFilter } : {}),
    ...(apiFormatFilter ? { api_format: apiFormatFilter } : {}),
    ...dateParams,
  };
  const { data: usage, isLoading, isError, refetch } = useUsage(params);
  const records = usage?.records ?? [];
  const total = usage?.total ?? 0;
  const page = offset / limit + 1;
  const totalPages = Math.max(1, Math.ceil(total / limit));
  const { data: models } = useQuery({
    queryKey: ['models'],
    queryFn: () => api<import('@/types').Model[]>('/models'),
    enabled: role === 'admin',
    retry: false,
  });
  const { currency, rate } = useCurrency();
  const [chartTab, setChartTab] = useState('list');
  const [chartDays, setChartDays] = useState(7);
  const { data: aggregate, isLoading: aggLoading } = useUsageAggregate(chartDays);
  const { data: modelActivity } = useModelActivity(chartDays);

  const modelPricing = useMemo(() => {
    if (!models) return {};
    const map: Record<string, { prompt_price: number; completion_price: number }> = {};
    for (const m of models) {
      map[m.name] = m.pricing;
    }
    return map;
  }, [models]);

  const handleChartTab = (tab: string) => {
    setChartTab(tab);
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('usage.title')}
        description={t('usage.subtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      <Tabs value={chartTab} onValueChange={handleChartTab}>
        <TabsList>
          <TabsTrigger value="list">
            <List className="size-4 mr-1" />{t('usage.list')}
          </TabsTrigger>
          <TabsTrigger value="chart">
            <BarChart3 className="size-4 mr-1" />{t('usage.chart')}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="list" className="space-y-4">
          {/* Collapsible filter bar */}
          <div className="flex items-center gap-2">
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
              {canFilterUsers && (
                <div className="relative">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                  <Input
                    className="pl-9" placeholder={t('usage.allUsers')}
                    value={userFilter} onChange={(e) => { setUserFilter(e.target.value); setOffset(0); }}
                  />
                </div>
              )}
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

          {/* Date range filter tabs */}
          {showFilters && (
            <div className="flex items-center gap-1 text-xs">
              {(['today', '7d', '30d', 'all'] as const).map((key) => (
                <button
                  key={key}
                  onClick={() => { setDateFilter(key); setOffset(0); }}
                  className={`px-2.5 py-1 rounded-md font-medium transition-colors ${
                    (!isCustomDate && dateFilter === key)
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
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="text-left py-3 px-4">{t('table.time')}</th>
                        <th className="text-left py-3 px-4">{t('table.requestId')}</th>
                        <th className="text-left py-3 px-4">{t('table.user')}</th>
                        <th className="text-left py-3 px-4">{t('table.apiKey')}</th>
                        <th className="text-left py-3 px-4">{t('table.model')}</th>
                        <th className="text-left py-3 px-4">{t('usage.apiFormat')}</th>
                        <th className="text-right py-3 px-4">{t('table.prompt')}</th>
                        <th className="text-right py-3 px-4">{t('table.cacheHit')}</th>
                        <th className="text-right py-3 px-4">{t('table.completion')}</th>
                        <th className="text-right py-3 px-4">{t('table.total')}</th>
                        {canFilterUsers && <th className="text-right py-3 px-4">{t('table.cost')}</th>}
                        <th className="text-right py-3 px-4">{t('table.latency')}</th>
                        <th className="text-center py-3 px-4">{t('table.status')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {records.map((r) => (
                        <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50 cursor-pointer" onClick={() => setDetailId(r.request_id)}>
                          <td className="py-3 px-4 text-muted-foreground whitespace-nowrap text-xs">
                            {new Date(r.timestamp).toLocaleString()}
                          </td>
                          <td className="py-3 px-4 font-mono text-xs">{r.request_id.substring(0, 8)}</td>
                          <td className="py-3 px-4">{r.user_name}</td>
                          <td className="py-3 px-4">{r.api_key_name}</td>
                          <td className="py-3 px-4">
                            <span className="inline-flex items-center gap-1">
                              <span>{r.model}</span>
                              {r.stream ? (
                                <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-yellow-600 bg-yellow-50 dark:text-yellow-400 dark:bg-yellow-950 px-1.5 py-0.5 rounded">
                                  <Radio className="h-2.5 w-2.5" />stream
                                </span>
                              ) : (
                                <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-950 px-1.5 py-0.5 rounded">
                                  <RadioIcon className="h-2.5 w-2.5" />sync
                                </span>
                              )}
                            </span>
                          </td>
                          <td className="py-3 px-4 font-mono text-xs">{r.api_format ?? '—'}</td>
                          <td className="py-3 px-4 text-right">{r.prompt_tokens}</td>
                          <td className="py-3 px-4 text-right text-muted-foreground">{r.cache_hit_input_tokens > 0 ? r.cache_hit_input_tokens : '—'}</td>
                          <td className="py-3 px-4 text-right">{r.completion_tokens}</td>
                          <td className="py-3 px-4 text-right font-medium">{r.total_tokens}</td>
                          {canFilterUsers && <td className="py-3 px-4 text-right font-mono text-xs">{formatCost(r.prompt_tokens, r.completion_tokens, getRecordPricing(r, modelPricing), currency, rate)}</td>}
                          <td className="py-3 px-4 text-right text-muted-foreground">{r.latency_ms}ms</td>
                          <td className="py-3 px-4 text-center">
                            {r.success ? (
                              <CheckCircle2 className="size-4 text-green-500 inline" />
                            ) : (
                              <XCircle className="size-4 text-red-500 inline" />
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
                <div className="flex items-center justify-between px-4 py-3 border-t">
                  <span className="text-xs text-muted-foreground">
                    {total > 0 && `${(page - 1) * limit + 1}–${Math.min(page * limit, total)} / ${total}`}
                  </span>
                  <div className="flex items-center gap-1">
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
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="chart" className="space-y-4">
          <div className="flex gap-2">
            {[7, 14, 30].map(d => (
              <Button key={d} variant={chartDays === d ? 'default' : 'outline'} size="sm" onClick={() => setChartDays(d)}>
                {d}{t('common.days')}
              </Button>
            ))}
          </div>

          {aggLoading ? (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground">{t('common.loading')}</CardContent>
            </Card>
          ) : aggregate && aggregate.length > 0 ? (
            <>
              <Card>
                <CardHeader><CardTitle className="text-base">{t('dash.requests')}</CardTitle></CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={250}>
                    <BarChart data={aggregate} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                      <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
                      <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} width={50} />
                      <Tooltip content={<ChartTooltip />} />
                      <Bar dataKey="count" fill="var(--chart-1)" radius={[4, 4, 0, 0]} name={t('dash.requests')} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              <Card>
                <CardHeader><CardTitle className="text-base">{t('usage.totalTokens')}</CardTitle></CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={250}>
                    <BarChart data={aggregate} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                      <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
                      <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} width={50} tickFormatter={(v: number) => v >= 1_000_000 ? `${(v / 1_000_000).toFixed(1)}M` : v >= 1_000 ? `${(v / 1_000).toFixed(1)}K` : `${v}`} />
                      <Tooltip content={<ChartTooltip />} />
                      <Legend
                        wrapperStyle={{ paddingTop: 8 }}
                        formatter={(value: string) => <span style={{ color: 'hsl(var(--foreground))', fontSize: 12 }}>{value}</span>}
                      />
                      <Bar dataKey="prompt_tokens" stackId="tokens" fill="var(--chart-2)" radius={[0, 0, 0, 0]} name={t('dash.prompt')} />
                      <Bar dataKey="cache_hit_tokens" stackId="tokens" fill="var(--chart-5)" radius={[0, 0, 0, 0]} name={t('usage.cacheHit')} />
                      <Bar dataKey="completion_tokens" stackId="tokens" fill="var(--chart-3)" radius={[4, 4, 0, 0]} name={t('dash.completion')} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              <Card>
                <CardHeader><CardTitle className="text-base">{t('dash.successRate')}</CardTitle></CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={200}>
                    <LineChart data={aggregate.map(d => ({ ...d, successRate: d.count > 0 ? +(d.success_count / d.count * 100).toFixed(1) : 100 }))} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                      <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
                      <YAxis domain={[0, 100]} unit="%" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} width={40} />
                      <Tooltip content={<ChartTooltip formatter={(value: number) => `${value}%`} />} />
                      <Line type="monotone" dataKey="successRate" stroke="hsl(142, 65%, 55%)" strokeWidth={2} dot={false} name={t('dash.successRate')} />
                    </LineChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              {modelActivity && modelActivity.length > 0 && (
                <>
                  {/* Model Activity — nav group engraved section header */}
                  <div className="px-1 pb-1">
                    <span className="text-sm font-semibold uppercase tracking-widest text-muted-foreground/35 select-none">
                      {t('usage.modelActivity')}
                    </span>
                  </div>

                  {/* Two bar charts side by side */}
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                    <Card>
                      <CardHeader><CardTitle className="text-base">{t('usage.modelUsage')}</CardTitle></CardHeader>
                      <CardContent>
                        <ResponsiveContainer width="100%" height={250}>
                          <BarChart data={modelActivity} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
                            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                            <XAxis dataKey="model" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} />
                            <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} width={55} tickFormatter={(v: number) => v >= 1_000_000 ? `${(v / 1_000_000).toFixed(1)}M` : v >= 1_000 ? `${(v / 1_000).toFixed(1)}K` : `${v}`} />
                            <Tooltip content={<ChartTooltip showTotal />} />
                            <Bar dataKey="prompt_tokens" stackId="tokens" fill="var(--chart-2)" radius={[0, 0, 0, 0]} name={t('dash.prompt')} />
                            <Bar dataKey="cache_hit_tokens" stackId="tokens" fill="var(--chart-5)" radius={[0, 0, 0, 0]} name={t('usage.cacheHit')} />
                            <Bar dataKey="completion_tokens" stackId="tokens" fill="var(--chart-3)" radius={[4, 4, 0, 0]} name={t('dash.completion')} />
                          </BarChart>
                        </ResponsiveContainer>
                      </CardContent>
                    </Card>

                    <Card>
                      <CardHeader><CardTitle className="text-base">{t('usage.modelSuccessRate')}</CardTitle></CardHeader>
                      <CardContent>
                        <ResponsiveContainer width="100%" height={250}>
                          <BarChart data={modelActivity} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
                            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                            <XAxis dataKey="model" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} />
                            <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} width={45} />
                            <Tooltip content={<ChartTooltip />} />
                            <Bar dataKey="success_count" stackId="status" fill="hsl(142, 65%, 55%)" name={t('usage.success')} />
                            <Bar dataKey="failure_count" stackId="status" fill="hsl(0, 70%, 55%)" name={t('usage.failure')} />
                          </BarChart>
                        </ResponsiveContainer>
                      </CardContent>
                    </Card>
                  </div>
                </>
              )}
            </>
          ) : (
            <Card>
              <CardContent className="p-8 text-center text-muted-foreground">{t('empty.noUsage')}</CardContent>
            </Card>
          )}
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
