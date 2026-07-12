import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useCurrency } from '@/store/currency';
import { formatCost } from '@/lib/cost';
import { useUsage, useUsageAggregate } from '@/api/usage';
import { api } from '@/api/client';
import { UsageLogDetail } from '@/components/UsageLogDetail';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Search, RefreshCw, CheckCircle2, XCircle, BarChart3, List, Radio, RadioIcon, Filter, ChevronDown, ChevronRight, X } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';
import {
  BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer,
  LineChart, Line, Legend,
} from 'recharts';

export default function Usage() {
  const { t } = useTranslation();
  const { role } = useAuth();
  const [limit, setLimit] = useState(50);
  const [offset, setOffset] = useState(0);
  const [showFilters, setShowFilters] = useState(false);
  const [userFilter, setUserFilter] = useState('');
  const [modelFilter, setModelFilter] = useState('');
  const [apiKeyFilter, setApiKeyFilter] = useState('');
  const [apiFormatFilter, setApiFormatFilter] = useState('');
  const [detailId, setDetailId] = useState<string | null>(null);
  const filtersActive = !!(role === 'admin' && userFilter) || modelFilter || apiKeyFilter || apiFormatFilter;
  const params = {
    limit, offset,
    ...(role === 'admin' && userFilter ? { user_id: userFilter } : {}),
    ...(modelFilter ? { model: modelFilter } : {}),
    ...(apiKeyFilter ? { api_key: apiKeyFilter } : {}),
    ...(apiFormatFilter ? { api_format: apiFormatFilter } : {}),
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
  const [chartDays, setChartDays] = useState(14);
  const { data: aggregate, isLoading: aggLoading } = useUsageAggregate(chartDays);

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
              {role === 'admin' && (
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
                        {role === 'admin' && <th className="text-right py-3 px-4">{t('table.cost')}</th>}
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
                          {role === 'admin' && <td className="py-3 px-4 text-right font-mono text-xs">{formatCost(r.prompt_tokens, r.completion_tokens, modelPricing[r.model], currency, rate)}</td>}
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
                    <BarChart data={aggregate}>
                      <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
                      <XAxis dataKey="date" fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <YAxis fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <Tooltip
                        contentStyle={{
                          background: 'hsl(var(--popover))',
                          border: '1px solid hsl(var(--border))',
                          borderRadius: 'var(--radius)',
                          fontSize: 13,
                        }}
                      />
                      <Bar dataKey="count" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} name={t('dash.requests')} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              <Card>
                <CardHeader><CardTitle className="text-base">{t('usage.totalTokens')}</CardTitle></CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={250}>
                    <BarChart data={aggregate}>
                      <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
                      <XAxis dataKey="date" fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <YAxis fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <Tooltip
                        contentStyle={{
                          background: 'hsl(var(--popover))',
                          border: '1px solid hsl(var(--border))',
                          borderRadius: 'var(--radius)',
                          fontSize: 13,
                        }}
                      />
                      <Legend />
                      <Bar dataKey="prompt_tokens" stackId="tokens" fill="hsl(215, 80%, 60%)" radius={[0, 0, 0, 0]} name={t('dash.prompt')} />
                      <Bar dataKey="completion_tokens" stackId="tokens" fill="hsl(140, 60%, 50%)" radius={[4, 4, 0, 0]} name={t('dash.completion')} />
                    </BarChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>

              <Card>
                <CardHeader><CardTitle className="text-base">{t('dash.successRate')}</CardTitle></CardHeader>
                <CardContent>
                  <ResponsiveContainer width="100%" height={200}>
                    <LineChart data={aggregate.map(d => ({ ...d, successRate: d.count > 0 ? +(d.success_count / d.count * 100).toFixed(1) : 100 }))}>
                      <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
                      <XAxis dataKey="date" fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <YAxis domain={[0, 100]} unit="%" fontSize={11} stroke="hsl(var(--muted-foreground))" />
                      <Tooltip
                        contentStyle={{
                          background: 'hsl(var(--popover))',
                          border: '1px solid hsl(var(--border))',
                          borderRadius: 'var(--radius)',
                          fontSize: 13,
                        }}
                        formatter={(value: number) => [`${value}%`, t('dash.successRate')]}
                      />
                      <Line type="monotone" dataKey="successRate" stroke="hsl(140, 60%, 50%)" strokeWidth={2} dot={false} name={t('dash.successRate')} />
                    </LineChart>
                  </ResponsiveContainer>
                </CardContent>
              </Card>
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
