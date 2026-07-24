import { useTranslation } from 'react-i18next';
import { Activity, BarChart3 } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import type { DashboardAggregations, TopModel } from '@/types';

type DashboardOverviewSectionProps = {
  aggregations: DashboardAggregations;
  currencySymbol: string;
  currencyCode: string;
  modelShare: TopModel[];
};

export function DashboardOverviewSection({
  aggregations,
  currencySymbol,
  currencyCode,
  modelShare,
}: DashboardOverviewSectionProps) {
  const { t } = useTranslation();

  return (
    <>
      <Card className="card-hover">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="size-4 text-brand" />
            {t('dash.usageOverview')}
          </CardTitle>
          <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <div>
              <p className="text-2xl font-bold">{currencySymbol}{aggregations.cost_24h.toFixed(2)}</p>
              <p className="mt-1 text-xs text-muted-foreground">{t('dash.cost24h')}</p>
              <p className="text-xs text-muted-foreground">{t('dash.cost24hLabel', { currency: currencyCode })}</p>
            </div>
            <div>
              <p className="text-2xl font-bold">{currencySymbol}{(aggregations.total_cost ?? 0).toFixed(2)}</p>
              <p className="mt-1 text-xs text-muted-foreground">{t('dash.historicalUsage')}</p>
              <p className="text-xs text-muted-foreground">{t('dash.totalCostLabel', { currency: currencyCode })}</p>
            </div>
            <div>
              <p className="text-2xl font-bold">{aggregations.total_requests.toLocaleString()}</p>
              <p className="mt-1 text-xs text-muted-foreground">{t('dash.requestCount')}</p>
              <p className="text-xs text-muted-foreground">{t('dash.totalRequests')}</p>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card className="card-hover">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Activity className="size-4 text-brand" />
            {t('dash.performance')}
          </CardTitle>
          <CardDescription>{t('dash.performanceSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <div className="rounded-lg bg-muted/50 p-3">
              <p className="mb-1 text-xs text-muted-foreground">{t('dash.successRate')}</p>
              <div className="flex items-center gap-2">
                <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
                  <div
                    className={`h-full rounded-full transition-all ${aggregations.success_rate_24h >= 95 ? 'bg-green-500' : aggregations.success_rate_24h >= 80 ? 'bg-yellow-500' : 'bg-red-500'}`}
                    style={{ width: `${Math.min(aggregations.success_rate_24h, 100)}%` }}
                  />
                </div>
                <span className="min-w-[3.5rem] text-right text-sm font-semibold">{aggregations.success_rate_24h.toFixed(1)}%</span>
              </div>
            </div>
            <div className="rounded-lg bg-muted/50 p-3">
              <p className="mb-1 text-xs text-muted-foreground">{t('dash.avgLatency')}</p>
              <p className="text-lg font-semibold">
                {aggregations.avg_latency_ms_24h >= 1000
                  ? `${(aggregations.avg_latency_ms_24h / 1000).toFixed(2)}s`
                  : `${aggregations.avg_latency_ms_24h.toFixed(0)}ms`}
              </p>
            </div>
            <div className="rounded-lg bg-muted/50 p-3">
              <p className="mb-1 text-xs text-muted-foreground">{t('dash.throughput')}</p>
              <p className="text-lg font-semibold">{aggregations.total_tokens_24h.toLocaleString()} tok</p>
            </div>
          </div>
        </CardContent>
      </Card>

      {modelShare.length > 0 && (
        <Card className="card-hover">
          <CardHeader>
            <CardTitle>{t('dash.topModels')}</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-2">
              {modelShare.map((item, index) => {
                const maxPercent = modelShare[0]?.percentage ?? 100;
                return (
                  <div key={item.model} className="flex items-center gap-3 py-1">
                    <span className="w-5 text-right text-xs text-muted-foreground">{index + 1}</span>
                    <span className="flex-1 truncate font-mono text-sm">{item.model}</span>
                    <div className="h-2 max-w-[200px] flex-1 overflow-hidden rounded-full bg-muted">
                      <div
                        className="h-full rounded-full transition-all"
                        style={{
                          width: `${(item.percentage / maxPercent) * 100}%`,
                          background: `var(--chart-${(index % 5) + 1})`,
                        }}
                      />
                    </div>
                    <span className="w-14 text-right text-xs font-medium">{item.percentage.toFixed(1)}%</span>
                    <span className="w-14 text-right text-xs text-muted-foreground">{t('common.count', { count: item.count })}</span>
                  </div>
                );
              })}
            </div>
          </CardContent>
        </Card>
      )}
    </>
  );
}
