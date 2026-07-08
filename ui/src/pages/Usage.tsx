import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useUsage } from '@/api/usage';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Search, RefreshCw, CheckCircle2, XCircle, BarChart3, List } from 'lucide-react';
import { toast } from 'sonner';

export default function Usage() {
  const { t } = useTranslation();
  const { role } = useAuth();
  const [limit, setLimit] = useState(50);
  const [userFilter, setUserFilter] = useState('');
  const params = role === 'admin' && userFilter ? { limit, user_id: userFilter } : { limit };
  const { data: usage, isLoading, refetch } = useUsage(params);
  const [chartTab, setChartTab] = useState('list');

  const handleChartTab = (tab: string) => {
    if (tab === 'chart') {
      fetch('/admin/api/usage/aggregations').catch(() => {
        toast.error('聚合 API 待实现，切换到列表视图');
        setChartTab('list');
        return;
      });
    }
    setChartTab(tab);
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('usage.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('usage.subtitle')}</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          <RefreshCw className="h-4 w-4 mr-1" />{t('common.refresh')}
        </Button>
      </div>

      <Tabs value={chartTab} onValueChange={handleChartTab}>
        <TabsList>
          <TabsTrigger value="list">
            <List className="h-4 w-4 mr-1" />列表
          </TabsTrigger>
          <TabsTrigger value="chart">
            <BarChart3 className="h-4 w-4 mr-1" />图表
          </TabsTrigger>
        </TabsList>

        <TabsContent value="list" className="space-y-4">
          {role === 'admin' && (
            <div className="flex gap-2">
              <div className="relative flex-1 max-w-xs">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
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
              ) : usage && usage.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="text-left py-3 px-4">{t('table.time')}</th>
                        <th className="text-left py-3 px-4">{t('table.requestId')}</th>
                        <th className="text-left py-3 px-4">{t('table.user')}</th>
                        <th className="text-left py-3 px-4">{t('table.model')}</th>
                        <th className="text-right py-3 px-4">{t('table.prompt')}</th>
                        <th className="text-right py-3 px-4">{t('table.completion')}</th>
                        <th className="text-right py-3 px-4">{t('table.total')}</th>
                        <th className="text-right py-3 px-4">{t('table.latency')}</th>
                        <th className="text-center py-3 px-4">{t('table.status')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {usage.map((r) => (
                        <tr key={r.request_id} className="border-b last:border-0 hover:bg-muted/50">
                          <td className="py-3 px-4 text-muted-foreground whitespace-nowrap text-xs">
                            {new Date(r.timestamp).toLocaleString()}
                          </td>
                          <td className="py-3 px-4 font-mono text-xs">{r.request_id.substring(0, 8)}</td>
                          <td className="py-3 px-4">{r.user_name}</td>
                          <td className="py-3 px-4">{r.model}</td>
                          <td className="py-3 px-4 text-right">{r.prompt_tokens}</td>
                          <td className="py-3 px-4 text-right">{r.completion_tokens}</td>
                          <td className="py-3 px-4 text-right font-medium">{r.total_tokens}</td>
                          <td className="py-3 px-4 text-right text-muted-foreground">{r.latency_ms}ms</td>
                          <td className="py-3 px-4 text-center">
                            {r.success ? (
                              <CheckCircle2 className="h-4 w-4 text-green-500 inline" />
                            ) : (
                              <XCircle className="h-4 w-4 text-red-500 inline" />
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
              <CardTitle className="text-base">用量图表</CardTitle>
            </CardHeader>
            <CardContent>
              <EmptyState message="聚合 API 待实现，切换到列表视图" />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
