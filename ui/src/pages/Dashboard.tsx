import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE } from '@/store/currency';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useSubscriptions } from '@/api/models';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Users, Radio, Braces, Key, Activity, Zap, BarChart3, Bell, HelpCircle } from 'lucide-react';

export default function Dashboard() {
  const { t } = useTranslation();
  const role = useAuth((s) => s.role);
  const { data: stats, isLoading } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: subscriptions } = useSubscriptions();
  const { currency, rate } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];
  const code = CURRENCY_CODE[currency];
  const convert = (v: number) => currency === 'cny' ? v * rate : v;
  const isAdmin = role === 'admin';

  const cards = isAdmin
    ? [
        { title: t('dash.users'), value: stats?.users ?? 0, icon: <Users className="h-5 w-5" /> },
        { title: t('dash.channels'), value: stats?.channels ?? 0, icon: <Radio className="h-5 w-5" /> },
        { title: t('dash.models'), value: stats?.models ?? 0, icon: <Braces className="h-5 w-5" /> },
        { title: t('dash.apiKeys'), value: stats?.api_keys ?? 0, icon: <Key className="h-5 w-5" /> },
        { title: t('dash.requests'), value: stats?.total_requests ?? 0, icon: <Activity className="h-5 w-5" /> },
      ]
    : [
        { title: t('dash.models'), value: subscriptions?.length ?? 0, icon: <Braces className="h-5 w-5" /> },
        { title: t('dash.apiKeys'), value: stats?.api_keys ?? 0, icon: <Key className="h-5 w-5" /> },
        { title: t('dash.requests'), value: stats?.total_requests ?? 0, icon: <Activity className="h-5 w-5" /> },
      ];

  return (
    <div className="space-y-6 animate-fade-in">
      <div>
        <h1 className="text-2xl font-semibold">{t('dash.title')}</h1>
        <p className="text-sm text-muted-foreground">{t('dash.subtitle')}</p>
      </div>

      <div className="grid grid-cols-2 md:grid-cols-3 gap-3">
        {cards.map((stat) => (
          <Card key={stat.title} className="p-4">
            <div className="flex items-center gap-3">
              <div className="p-2 rounded-lg bg-brand/10 text-brand">{stat.icon}</div>
              <div>
                <p className="text-xs text-muted-foreground">{stat.title}</p>
                <p className="text-xl font-semibold mt-0.5">{isLoading ? '...' : stat.value.toLocaleString()}</p>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {agg && (
        <>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <BarChart3 className="h-4 w-4 text-brand" />
                {t('dash.usageOverview')}
              </CardTitle>
              <p className="text-xs text-muted-foreground">{t('dash.usageOverviewSub')}</p>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-6">
                <div>
                  <p className="text-2xl font-bold">{sym}{convert(agg.cost_24h).toFixed(2)}</p>
                  <p className="text-xs text-muted-foreground mt-1">{t('dash.cost24h')}</p>
                  <p className="text-xs text-muted-foreground">{t('dash.cost24hLabel', { currency: code })}</p>
                </div>
                <div>
                  <p className="text-2xl font-bold">{sym}{convert(agg.total_cost ?? 0).toFixed(2)}</p>
                  <p className="text-xs text-muted-foreground mt-1">{t('dash.historicalUsage')}</p>
                  <p className="text-xs text-muted-foreground">{t('dash.totalCostLabel', { currency: code })}</p>
                </div>
                <div>
                  <p className="text-2xl font-bold">{agg.total_requests.toLocaleString()}</p>
                  <p className="text-xs text-muted-foreground mt-1">{t('dash.requestCount')}</p>
                  <p className="text-xs text-muted-foreground">{t('dash.totalRequests')}</p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <Zap className="h-4 w-4 text-brand" />
                {t('dash.performance')}
              </CardTitle>
              <p className="text-xs text-muted-foreground">{t('dash.performanceSub')}</p>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
                <div className="p-3 rounded-lg bg-muted/50">
                  <p className="text-xs text-muted-foreground mb-1">{t('dash.successRate')}</p>
                  <div className="flex items-center gap-2">
                    <div className="flex-1 h-2 rounded-full bg-muted overflow-hidden">
                      <div
                        className={`h-full rounded-full transition-all ${agg.success_rate_24h >= 95 ? 'bg-green-500' : agg.success_rate_24h >= 80 ? 'bg-yellow-500' : 'bg-red-500'}`}
                        style={{ width: `${Math.min(agg.success_rate_24h, 100)}%` }}
                      />
                    </div>
                    <span className="text-sm font-semibold min-w-[3.5rem] text-right">{agg.success_rate_24h.toFixed(1)}%</span>
                  </div>
                </div>
                <div className="p-3 rounded-lg bg-muted/50">
                  <p className="text-xs text-muted-foreground mb-1">{t('dash.avgLatency')}</p>
                  <p className="text-lg font-semibold">
                    {agg.avg_latency_ms_24h >= 1000
                      ? `${(agg.avg_latency_ms_24h / 1000).toFixed(2)}s`
                      : `${agg.avg_latency_ms_24h.toFixed(0)}ms`}
                  </p>
                </div>
                <div className="p-3 rounded-lg bg-muted/50">
                  <p className="text-xs text-muted-foreground mb-1">{t('dash.throughput')}</p>
                  <p className="text-lg font-semibold">{agg.total_tokens_24h.toLocaleString()} tok</p>
                </div>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm flex items-center gap-2">
                <BarChart3 className="h-4 w-4 text-brand" />
                {t('dash.topModels')}
              </CardTitle>
            </CardHeader>
            <CardContent>
              {agg.top_models_24h.length > 0 ? (
                <div className="space-y-2">
                  {agg.top_models_24h.map((m, i) => {
                    const maxPercent = agg.top_models_24h[0]?.percentage ?? 100;
                    return (
                      <div key={m.model} className="flex items-center gap-3 py-1">
                        <span className="text-xs text-muted-foreground w-5 text-right">{i + 1}</span>
                        <span className="text-sm flex-1 truncate font-mono">{m.model}</span>
                        <div className="flex-1 h-2 rounded-full bg-muted overflow-hidden max-w-[200px]">
                          <div
                            className="h-full rounded-full bg-brand transition-all"
                            style={{ width: `${(m.percentage / maxPercent) * 100}%` }}
                          />
                        </div>
                        <span className="text-xs font-medium w-14 text-right">{m.percentage.toFixed(1)}%</span>
                        <span className="text-xs text-muted-foreground w-14 text-right">{t('common.count', { count: m.count })}</span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground py-4 text-center">{t('dash.noData')}</p>
              )}
            </CardContent>
          </Card>
        </>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm flex items-center gap-2"><Bell className="h-4 w-4" /> {t('dash.announcements')}</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-xs text-muted-foreground">{t('dash.announcementsSub')}</p>
            <p className="text-sm text-muted-foreground mt-3">{t('dash.noAnnouncements')}</p>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm flex items-center gap-2"><HelpCircle className="h-4 w-4" /> FAQ</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-xs text-muted-foreground">{t('dash.faqSub')}</p>
            <p className="text-sm text-muted-foreground mt-3">{t('dash.noFaq')}</p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
