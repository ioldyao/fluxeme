import { useTranslation } from 'react-i18next';
import {
  Area, AreaChart, Pie, PieChart, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis, CartesianGrid,
} from 'recharts';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@/components/ui/button';
import { usePermission } from '@/permissions';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE } from '@/store/currency';
import { useDashboard, useDashboardAggregations, useDailyUsage } from '@/api/dashboard';
import { useSubscriptions } from '@/api/models';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import {
  Users, Radio, Braces, Key, Activity, BarChart3, Bell, HelpCircle,
} from 'lucide-react';

function ChartTooltip({ active, payload, label }: any) {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-lg border bg-popover px-3 py-2 text-xs shadow-md">
      {label && <p className="mb-1 font-medium text-popover-foreground">{label}</p>}
      {payload.map((entry: any, i: number) => (
        <div key={i} className="flex items-center gap-2 text-muted-foreground">
          <span className="size-2 rounded-full" style={{ background: entry.color }} />
          <span>{entry.name}</span>
          <span className="ml-auto font-mono font-medium text-popover-foreground">
            {typeof entry.value === 'number' ? entry.value.toLocaleString() : entry.value}
          </span>
        </div>
      ))}
    </div>
  );
}

const CHART_COLORS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'];

export default function Dashboard() {
  const { t } = useTranslation();
  const { data: stats, isLoading, isError, refetch } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: dailyData } = useDailyUsage(14);
  const { data: subscriptions } = useSubscriptions();
  const { currency, rate } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];
  const code = CURRENCY_CODE[currency];
  const convert = (v: number) => (currency === 'cny' ? v * rate : v);
  const isAdmin = usePermission('admin:dashboard');

  const cards = isAdmin
    ? [
      { title: t('dash.users'), value: stats?.users ?? 0, icon: <Users className="size-5" /> },
      { title: t('dash.channels'), value: stats?.channels ?? 0, icon: <Radio className="size-5" /> },
      { title: t('dash.models'), value: stats?.models ?? 0, icon: <Braces className="size-5" /> },
      { title: t('dash.apiKeys'), value: stats?.api_keys ?? 0, icon: <Key className="size-5" /> },
      { title: t('dash.requests'), value: stats?.total_requests ?? 0, icon: <Activity className="size-5" /> },
    ]
    : [
      { title: t('dash.models'), value: subscriptions?.length ?? 0, icon: <Braces className="size-5" /> },
      { title: t('dash.apiKeys'), value: stats?.api_keys ?? 0, icon: <Key className="size-5" /> },
      { title: t('dash.requests'), value: stats?.total_requests ?? 0, icon: <Activity className="size-5" /> },
    ];

  const modelShare = agg?.top_models_24h?.slice(0, 5) ?? [];

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader title={t('dash.title')} description={t('dash.subtitle')} />

      {isError ? (
        <div className="flex items-center justify-center p-8">
          <div className="text-center">
            <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
            <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
          </div>
        </div>
      ) : (
        <>
          {/* Stats cards */}
          <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-4">
            {cards.map((stat) => (
              <Card key={stat.title}>
                <CardContent className="flex items-center gap-3 p-5">
                  <div className="p-2 rounded-lg bg-brand/10 text-brand shrink-0">{stat.icon}</div>
                  <div className="min-w-0">
                    <p className="text-xs text-muted-foreground truncate">{stat.title}</p>
                    <p className="text-xl font-semibold mt-0.5">{isLoading ? '...' : stat.value.toLocaleString()}</p>
                  </div>
                </CardContent>
              </Card>
            ))}
          </div>

      {agg && (
        <>
          {/* Charts row */}
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
            {/* Request trend */}
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <BarChart3 className="size-4 text-brand" />
                  {t('dash.requests')}
                </CardTitle>
                <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
              </CardHeader>
              <CardContent>
                {dailyData && dailyData.length > 0 ? (
                  <ResponsiveContainer width="100%" height={260}>
                    <AreaChart data={dailyData} margin={{ left: -12, right: 8, top: 4 }}>
                      <defs>
                        <linearGradient id="reqFill" x1="0" y1="0" x2="0" y2="1">
                          <stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.35} />
                          <stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} />
                        </linearGradient>
                      </defs>
                      <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                      <XAxis
                        dataKey="date"
                        tickLine={false}
                        axisLine={false}
                        tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                        tickFormatter={(v) => v.slice(5)}
                      />
                      <YAxis
                        tickLine={false}
                        axisLine={false}
                        tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                      />
                      <Tooltip content={<ChartTooltip />} />
                      <Area
                        type="monotone"
                        dataKey="count"
                        name={t('dash.requests')}
                        stroke="var(--chart-1)"
                        strokeWidth={2}
                        fill="url(#reqFill)"
                      />
                    </AreaChart>
                  </ResponsiveContainer>
                ) : (
                  <p className="text-sm text-muted-foreground py-8 text-center">{t('dash.noData')}</p>
                )}
              </CardContent>
            </Card>

            {/* Model usage share */}
            <Card>
              <CardHeader>
                <CardTitle className="flex items-center gap-2">
                  <Braces className="size-4 text-brand" />
                  {t('dash.topModels')}
                </CardTitle>
                <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
              </CardHeader>
              <CardContent>
                {modelShare.length > 0 ? (
                  <>
                    <ResponsiveContainer width="100%" height={180}>
                      <PieChart>
                        <Pie
                          data={modelShare}
                          dataKey="count"
                          nameKey="model"
                          innerRadius={48}
                          outerRadius={72}
                          paddingAngle={2}
                          strokeWidth={0}
                        >
                          {modelShare.map((_, i) => (
                            <Cell key={i} fill={CHART_COLORS[i % CHART_COLORS.length]} />
                          ))}
                        </Pie>
                        <Tooltip content={<ChartTooltip />} />
                      </PieChart>
                    </ResponsiveContainer>
                    <div className="mt-2 space-y-1.5">
                      {modelShare.map((m, i) => (
                        <div key={m.model} className="flex items-center gap-2 text-sm">
                          <span className="size-2.5 rounded-full shrink-0" style={{ background: CHART_COLORS[i] }} />
                          <span className="text-muted-foreground truncate flex-1">{m.model}</span>
                          <span className="font-medium">{m.percentage.toFixed(1)}%</span>
                        </div>
                      ))}
                    </div>
                  </>
                ) : (
                  <p className="text-sm text-muted-foreground py-8 text-center">{t('dash.noData')}</p>
                )}
              </CardContent>
            </Card>
          </div>

          {/* Cost overview */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <BarChart3 className="size-4 text-brand" />
                {t('dash.usageOverview')}
              </CardTitle>
              <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
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

          {/* Performance */}
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2">
                <Activity className="size-4 text-brand" />
                {t('dash.performance')}
              </CardTitle>
              <CardDescription>{t('dash.performanceSub')}</CardDescription>
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

          {/* Top models list */}
          {modelShare.length > 0 && (
            <Card>
              <CardHeader>
                <CardTitle>{t('dash.topModels')}</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="space-y-2">
                  {modelShare.map((m, i) => {
                    const maxPercent = modelShare[0]?.percentage ?? 100;
                    return (
                      <div key={m.model} className="flex items-center gap-3 py-1">
                        <span className="text-xs text-muted-foreground w-5 text-right">{i + 1}</span>
                        <span className="text-sm flex-1 truncate font-mono">{m.model}</span>
                        <div className="flex-1 h-2 rounded-full bg-muted overflow-hidden max-w-[200px]">
                          <div
                            className="h-full rounded-full transition-all"
                            style={{ width: `${(m.percentage / maxPercent) * 100}%`, background: CHART_COLORS[i] }}
                          />
                        </div>
                        <span className="text-xs font-medium w-14 text-right">{m.percentage.toFixed(1)}%</span>
                        <span className="text-xs text-muted-foreground w-14 text-right">{t('common.count', { count: m.count })}</span>
                      </div>
                    );
                  })}
                </div>
              </CardContent>
            </Card>
          )}
        </>
      )}

      {/* Info cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2"><Bell className="size-4" /> {t('dash.announcements')}</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-xs text-muted-foreground">{t('dash.announcementsSub')}</p>
            <p className="text-sm text-muted-foreground mt-3">{t('dash.noAnnouncements')}</p>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2"><HelpCircle className="size-4" /> FAQ</CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-xs text-muted-foreground">{t('dash.faqSub')}</p>
            <p className="text-sm text-muted-foreground mt-3">{t('dash.noFaq')}</p>
          </CardContent>
        </Card>
      </div>
        </>
      )}
    </div>
  );
}
