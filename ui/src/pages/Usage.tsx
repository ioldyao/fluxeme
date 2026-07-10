import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useCurrency } from '@/store/currency';
import { formatCost } from '@/lib/cost';
import { useUsage } from '@/api/usage';
import { api } from '@/api/client';
import { UsageLogDetail } from '@/components/UsageLogDetail';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Search, RefreshCw, CheckCircle2, XCircle, BarChart3, List } from 'lucide-react';
import { useQuery } from '@tanstack/react-query';

export default function Usage() {
  const { t } = useTranslation();
  const { role } = useAuth();
  const [limit, setLimit] = useState(50);
  const [userFilter, setUserFilter] = useState('');
  const [detailId, setDetailId] = useState<string | null>(null);
  const params = role === 'admin' && userFilter ? { limit, user_id: userFilter } : { limit };
  const { data: usage, isLoading, isError, refetch } = useUsage(params);
  const { data: models } = useQuery({
    queryKey: ['models'],
    queryFn: () => api<import('@/types').Model[]>('/models'),
    enabled: role === 'admin',
    retry: false,
  });
  const { currency, rate } = useCurrency();
  const [chartTab, setChartTab] = useState('list');

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
          {role === 'admin' && (
            <div className="flex gap-2">
              <div className="relative flex-1 max-w-xs">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
                <Input
                  className="pl-9" placeholder={t('usage.allUsers')}
                  value={userFilter} onChange={(e) => setUserFilter(e.target.value)}
                />
              </div>
              <Input type="number" className="w-20" value={limit}
                onChange={(e) => setLimit(Number(e.target.value) || 50)} />
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
              ) : usage && usage.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="text-left py-3 px-4">{t('table.time')}</th>
                        <th className="text-left py-3 px-4">{t('table.requestId')}</th>
                        <th className="text-left py-3 px-4">{t('table.user')}</th>
                        <th className="text-left py-3 px-4">{t('table.apiKey')}</th>
                        <th className="text-left py-3 px-4">{t('table.model')}</th>
                        <th className="text-right py-3 px-4">{t('table.prompt')}</th>
                        <th className="text-right py-3 px-4">{t('table.completion')}</th>
                        <th className="text-right py-3 px-4">{t('table.total')}</th>
                        {role === 'admin' && <th className="text-right py-3 px-4">{t('table.cost')}</th>}
                        <th className="text-right py-3 px-4">{t('table.latency')}</th>
                        <th className="text-center py-3 px-4">{t('table.status')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {usage.map((r) => (
                        <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50 cursor-pointer" onClick={() => setDetailId(r.request_id)}>
                          <td className="py-3 px-4 text-muted-foreground whitespace-nowrap text-xs">
                            {new Date(r.timestamp).toLocaleString()}
                          </td>
                          <td className="py-3 px-4 font-mono text-xs">{r.request_id.substring(0, 8)}</td>
                          <td className="py-3 px-4">{r.user_name}</td>
                          <td className="py-3 px-4">{r.api_key_name}</td>
                          <td className="py-3 px-4">{r.model}</td>
                          <td className="py-3 px-4 text-right">{r.prompt_tokens}</td>
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
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="chart">
          <Card>
            <CardHeader>
              <CardTitle>{t('usage.chart')}</CardTitle>
            </CardHeader>
            <CardContent>
              <EmptyState message={t('usage.chartNotAvailable')} />
            </CardContent>
          </Card>
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
