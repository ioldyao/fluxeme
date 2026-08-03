// Importers/callers: mounted by user-app/src/routes/config.ts and src/routes/index.tsx
// as the authenticated dashboard page. Affected APIs: GET /dashboard,
// /dashboard/aggregations, /usage, /usage/aggregate, /usage/model-activity,
// /usage/funnel, /wallet/overview, /wallet/estimated-days. Data schemas: the
// component consumes DashboardStats, DashboardAggregations, UsageResponse,
// DailyAggregate[], ModelActivity[], FunnelStats, WalletOverview, and
// { days: number | null }. User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Activity, AlertTriangle, Bell, HelpCircle, Info, Layers3, ShieldCheck, Wallet,
} from 'lucide-react';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader } from '@fluxeme/shared/src/components/ui/card';
import { useCurrency, CURRENCY_SYMBOL } from '@fluxeme/shared/src/store/currency';
import { useSelfDashboard, useSelfDashboardAggregations } from '@fluxeme/shared/src/api/dashboard';
import { useMyUsage, useMyUsageAggregate, useMyModelActivity, useMyUsageFunnel } from '@fluxeme/shared/src/api/usage';
import { useEstimatedDays, useWalletOverview } from '@fluxeme/shared/src/api/wallet';
import { usePublishedAnnouncements } from '@fluxeme/shared/src/api/announcements';
import { DashboardChartTooltip } from '@fluxeme/shared/src/components/dashboard/DashboardChartTooltip';
import {
  Area, AreaChart, CartesianGrid, Cell, Pie, PieChart,
  ResponsiveContainer, Tooltip, XAxis, YAxis,
} from 'recharts';

const RANGE_DAYS = [1, 7, 30] as const;
const CHART_COLORS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'];
const CHART_OPTS = ['token', '请求', '错误率'] as const;

function fmt(sym: string, v?: number) {
  const a = v ?? 0;
  return `${sym}${a.toFixed(2)}`;
}

function fmtLat(ms: number) {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms.toFixed(0)}ms`;
}

export default function Dashboard() {
  const { t } = useTranslation();
  const [days, setDays] = useState(1);
  const [chartOpt, setChartOpt] = useState<string>(CHART_OPTS[0]);

  const {
    data: stats,
    isError: statsErr,
    isLoading: statsLoading,
    refetch,
  } = useSelfDashboard();
  const {
    data: agg,
    isError: aggErr,
    isLoading: aggLoading,
    refetch: ra,
  } = useSelfDashboardAggregations();
  const {
    data: ua,
    isError: usageAggregateError,
    isLoading: usageAggregateLoading,
    isPlaceholderData: isUsageAggregatePlaceholder,
    refetch: rua,
  } = useMyUsageAggregate(days);
  const {
    data: ma,
    isLoading: modelActivityLoading,
    refetch: rma,
  } = useMyModelActivity(days);
  const { data: recent, refetch: rrl } = useMyUsage({ limit: 8 });
  const { data: wo, refetch: rwo } = useWalletOverview();
  const { data: ed, refetch: red } = useEstimatedDays();
  const {
    data: funnel,
    isError: funnelError,
    isLoading: funnelLoading,
    isPlaceholderData: isFunnelPlaceholder,
    refetch: rfunnel,
  } = useMyUsageFunnel(days);
  const { currency } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];
  const { data: announcements } = usePublishedAnnouncements();

  const isHealthStripLoading = statsLoading || aggLoading;
  const availability = agg?.success_rate_24h ?? 0;
  const avgLat = agg?.avg_latency_ms_24h ?? 0;
  const apiKeyCount = stats?.api_keys ?? 0;
  const requests24h = agg?.requests_24h ?? 0;
  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const selectedPeriodTokens = useMemo(() => {
    if (!ua) return days === 1 ? totalTokens24h : 0;
    return ua.reduce((sum, day) => sum + day.total_tokens, 0);
  }, [days, totalTokens24h, ua]);
  const isRequestFlowLoading = usageAggregateLoading
    || funnelLoading
    || isUsageAggregatePlaceholder
    || isFunnelPlaceholder;
  const hasRequestFlowData = !!funnel && (days === 1 || !!ua);
  const hasRequestFlowError = !hasRequestFlowData && (funnelError || (days !== 1 && usageAggregateError));
  const gatewayError = statsErr || aggErr;
  const toneCls = isHealthStripLoading
    ? 'bg-muted-foreground/30 shadow-none'
    : gatewayError
      ? 'bg-red-500 shadow-[0_0_0_6px_rgba(216,75,75,0.14)]'
      : availability >= 99
        ? 'bg-emerald-500 shadow-[0_0_0_6px_rgba(20,150,106,0.12)]'
        : availability >= 95
          ? 'bg-amber-500 shadow-[0_0_0_6px_rgba(217,145,19,0.14)]'
          : 'bg-emerald-500 shadow-[0_0_0_6px_rgba(20,150,106,0.12)]';
  const toneLabel = isHealthStripLoading ? t('common.loading') : gatewayError ? t('gateway.unstable') : t('gateway.healthy');

  // model share
  const modelShare = useMemo(() => {
    if (!ma?.length) return [];
    const sorted = ma.slice().sort((a, b) => b.total_requests - a.total_requests);
    const top5 = sorted.slice(0, 5);
    const total = sorted.reduce((s, i) => s + i.total_requests, 0);
    const items = top5.map(i => ({ model: i.model, count: i.total_requests, percentage: total > 0 ? (i.total_requests / total) * 100 : 0 }));
    const rem = total - top5.reduce((s, i) => s + i.total_requests, 0);
    if (rem > 0) items.push({ model: t('dash.otherModels'), count: rem, percentage: (rem / total) * 100 });
    return items;
  }, [ma, t]);

  // alerts
  const alerts = useMemo(() => {
    const a: { id: string; title: string; desc: string; warn: boolean }[] = [];
    if (agg && avgLat > 2000) a.push({ id: 'lat', title: t('dash.alertLatencyTitle'), desc: t('dash.alertLatencyDesc', { latency: avgLat.toFixed(0) }), warn: true });
    if (agg && availability < 95) a.push({ id: 'suc', title: t('dash.alertSuccessTitle'), desc: t('dash.alertSuccessDesc', { rate: availability.toFixed(1) }), warn: true });
    if ((modelShare[0]?.percentage ?? 0) > 80) a.push({ id: 'con', title: t('dash.alertConcentrationTitle'), desc: t('dash.alertConcentrationDesc', { model: modelShare[0]?.model ?? '—', share: (modelShare[0]?.percentage ?? 0).toFixed(1) }), warn: false });
    if ((ed?.days ?? Infinity) < 10) a.push({ id: 'bal', title: t('dash.alertBalanceTitle'), desc: t('dash.alertBalanceDesc', { days: (ed?.days ?? 0).toFixed(1) }), warn: true });
    return a;
  }, [agg, avgLat, availability, modelShare, ed?.days, t]);

  const handleRefresh = () => {
    void refetch();
    void ra();
    void rua();
    void rma();
    void rrl();
    void rwo();
    void red();
    void rfunnel();
  };

  const chartData = useMemo(() => {
    if (!ua?.length) return [];
    return ua.map(d => ({
      date: d.date.slice(5),
      requests: d.count,
      total_tokens: d.total_tokens,
      errors: d.count - d.success_count,
    }));
  }, [ua]);

  return (
    <div className="space-y-5 animate-fade-in">
      {/* Page head: title + range */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{t('dash.opsTitle')}</h1>
          <p className="mt-1.5 text-sm text-muted-foreground">{t('dash.opsSubtitle')}</p>
        </div>
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1 rounded-lg border bg-card p-1 shadow-sm">
            {RANGE_DAYS.map(d => (
              <button key={d} type="button" onClick={() => setDays(d)}
                className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${d === days ? 'bg-amber-500/15 text-amber-700 font-semibold' : 'text-muted-foreground hover:text-foreground'}`}
              >{d === 1 ? '24H' : `${d}D`}</button>
            ))}
          </div>
          <Button variant="outline" size="sm" onClick={handleRefresh}>
            <Activity className="mr-1 size-3.5" />{t('common.refresh')}
          </Button>
        </div>
      </div>

      {/* Health Strip */}
      <section className="grid grid-cols-1 gap-3 xl:grid-cols-[1.4fr_repeat(4,minmax(0,1fr))]">
        <div className="rounded-xl border bg-card p-5 shadow-sm">
          <div className="flex items-center justify-between gap-4">
            <div className="flex items-center gap-3">
              <span className={`size-3 rounded-full ${toneCls}`} aria-hidden="true" />
              <div>
                <div className="font-semibold text-foreground">{toneLabel}</div>
                <div className="mt-1 text-sm text-muted-foreground">{t('dash.myKeys')}: {isHealthStripLoading ? '—' : apiKeyCount}</div>
              </div>
            </div>
            <div className="text-right">
              <div className="text-2xl font-semibold tracking-tight">{isHealthStripLoading || gatewayError ? '—' : requests24h > 0 ? `${availability.toFixed(2)}%` : '—'}</div>
              <div className="text-xs text-muted-foreground">{t('dash.availability')}</div>
            </div>
          </div>
        </div>
        {[
          { title: t('dash.requests'), val: isHealthStripLoading ? '—' : requests24h.toLocaleString(), icon: <Activity className="size-4" /> },
          { title: t('usage.totalTokens'), val: isHealthStripLoading ? '—' : totalTokens24h.toLocaleString(), icon: <Layers3 className="size-4" /> },
          { title: t('dash.avgLatency'), val: isHealthStripLoading ? '—' : fmtLat(avgLat), icon: <ShieldCheck className="size-4" /> },
          { title: t('dash.cost24h'), val: isHealthStripLoading ? '—' : fmt(sym, agg?.cost_24h), icon: <Wallet className="size-4" /> },
        ].map(m => (
          <div key={m.title} className="rounded-xl border bg-card p-4 shadow-sm">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="rounded-md bg-brand/10 p-1 text-brand">{m.icon}</span> {m.title}
            </div>
            <div className="mt-3 text-2xl font-semibold tracking-tight">{m.val}</div>
          </div>
        ))}
      </section>

      {/* Dashboard Grid: left main + right rail */}
      <section className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.75fr)_minmax(310px,0.8fr)]" style={{ alignItems: 'start' }}>

        {/* ── Left Column ── */}
        <div className="space-y-4">
          {/* Request Flow Funnel (Scheme B) */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="text-base font-semibold leading-none">{t('dash.requestFlow')}</h2>
              <CardDescription>{t('dash.requestFlowSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              {isRequestFlowLoading ? (
                <div className="grid grid-cols-1 gap-4 sm:grid-cols-3 md:grid-cols-5">
                  {Array.from({ length: 5 }, (_, index) => (
                    <div key={index} className="h-48 animate-pulse rounded-lg border bg-muted/20" />
                  ))}
                </div>
              ) : hasRequestFlowError || !hasRequestFlowData ? (
                <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
              ) : (() => {
                const f = funnel;
                if (!f) {
                  return <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>;
                }
                const total = f.total;
                const successCount = f.success_count;
                const authCount = f.auth_fail_count;
                const rateLimitCount = f.rate_limit_count;
                const badReqCount = f.bad_request_count;
                const upstreamErrCount = f.upstream_error_count;
                const timeoutCount = f.timeout_count;
                const otherErrCount = f.other_error_count;
                const totalTokens = selectedPeriodTokens;
                const intervalSecs = days * 86400;
                const avgQps = intervalSecs > 0 ? (total / intervalSecs) : 0;
                const avgTps = intervalSecs > 0 ? (totalTokens / intervalSecs) : 0;
                const bizLimits = rateLimitCount + badReqCount + authCount;
                const slaEligibleTotal = Math.max(0, total - bizLimits);
                const slaLvl = slaEligibleTotal > 0 ? (successCount / slaEligibleTotal) * 100 : 0;
                const requestSuccessRate = total > 0 ? (successCount / total) * 100 : 0;
                const requestErrorRate = total > 0 ? 100 - requestSuccessRate : 0;
                const sysErrors = upstreamErrCount + timeoutCount;
                const totalErrors = sysErrors + bizLimits + otherErrCount;
                const healthy = totalErrors === 0;
                const p50s = f.p50_latency / 1000;
                const p95s = f.p95_latency / 1000;
                const p99s = f.p99_latency / 1000;
                const avgS = f.avg_latency / 1000;
                return (
                  <div className="grid grid-cols-1 gap-4 sm:grid-cols-3 md:grid-cols-5">
                    {/* Stage 01 */}
                    <div className="rounded-lg border bg-muted/20 p-4">
                      <div className="text-[11px] font-medium text-muted-foreground">阶段 01</div>
                      <h3 className="mt-1.5 text-sm font-semibold text-foreground">请求入口</h3>
                      <div className="mt-4 text-2xl font-semibold tracking-tight">{total.toLocaleString()}</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">请求数</div>
                      <div className="mt-3 text-xl font-semibold tracking-tight">{totalTokens >= 1000000 ? `${(totalTokens / 1000000).toFixed(1)}M` : totalTokens >= 1000 ? `${(totalTokens / 1000).toFixed(1)}K` : totalTokens.toLocaleString()}</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">Token 数</div>
                      <div className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                        <div><span className="text-muted-foreground">平均 QPS</span><div className="font-semibold">{avgQps.toFixed(1)}</div></div>
                        <div><span className="text-muted-foreground">平均 TPS</span><div className="font-semibold">{avgTps.toFixed(1)}</div></div>
                      </div>
                    </div>

                    {/* Stage 02 */}
                    <div className="rounded-lg border bg-muted/20 p-4">
                      <div className="text-[11px] font-medium text-muted-foreground">阶段 02</div>
                      <h3 className="mt-1.5 text-sm font-semibold text-foreground">网关处理</h3>
                      <div className="mt-4 text-2xl font-semibold tracking-tight">{slaLvl.toFixed(3)}%</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">SLA · 排除业务限制</div>
                      <div className="mt-3 space-y-1.5 text-[11px]">
                        <div className="flex justify-between"><span className="text-muted-foreground">系统异常</span><b className={sysErrors > 0 ? 'text-red-500' : ''}>{sysErrors}</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">业务限制</span><b>{bizLimits}</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">健康状态</span><b className={healthy ? 'text-emerald-600' : 'text-amber-600'}>{healthy ? '正常' : '异常'}</b></div>
                      </div>
                    </div>

                    {/* Stage 03 */}
                    <div className="rounded-lg border bg-muted/20 p-4">
                      <div className="text-[11px] font-medium text-muted-foreground">阶段 03</div>
                      <h3 className="mt-1.5 text-sm font-semibold text-foreground">请求延迟</h3>
                      <div className="mt-4 text-2xl font-semibold tracking-tight">{p50s.toFixed(2)}s</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">端到端 · P50</div>
                      <div className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                        <div><span className="text-muted-foreground">P95</span><div className="font-semibold">{p95s.toFixed(2)}s</div></div>
                        <div><span className="text-muted-foreground">P99</span><div className="font-semibold">{p99s.toFixed(2)}s</div></div>
                        <div className="col-span-2"><span className="text-muted-foreground">平均</span><div className="font-semibold">{avgS.toFixed(2)}s</div></div>
                      </div>
                    </div>

                    {/* Stage 04 */}
                    <div className="rounded-lg border bg-muted/20 p-4">
                      <div className="text-[11px] font-medium text-muted-foreground">阶段 04</div>
                      <h3 className="mt-1.5 text-sm font-semibold text-foreground">模型执行</h3>
                      <div className="mt-4 text-2xl font-semibold tracking-tight">{avgS.toFixed(2)}s</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">平均请求时长</div>
                      <div className="mt-3 grid grid-cols-2 gap-2 text-[11px]">
                        <div><span className="text-muted-foreground">P95</span><div className="font-semibold">{p95s.toFixed(2)}s</div></div>
                        <div><span className="text-muted-foreground">P99</span><div className="font-semibold">{p99s.toFixed(2)}s</div></div>
                      </div>
                      <div className="mt-3 space-y-1 text-[11px]">
                        <div className="flex justify-between"><span className="text-muted-foreground">排队中</span><b>{rateLimitCount}</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">发生重试</span><b>0</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">上下文超限</span><b>{badReqCount}</b></div>
                      </div>
                    </div>

                    {/* Stage 05 */}
                    <div className="rounded-lg border bg-muted/20 p-4">
                      <div className="text-[11px] font-medium text-muted-foreground">阶段 05</div>
                      <h3 className="mt-1.5 text-sm font-semibold text-foreground">最终结果</h3>
                      <div className="mt-4 text-2xl font-semibold tracking-tight">{requestSuccessRate.toFixed(2)}%</div>
                      <div className="mt-1 text-[11px] text-muted-foreground">请求成功率</div>
                      <div className="mt-3 space-y-1.5 text-[11px]">
                        <div className="flex justify-between"><span className="text-muted-foreground">请求错误率</span><b className="text-red-500">{requestErrorRate.toFixed(2)}%</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">错误请求</span><b className="text-red-500">{totalErrors}</b></div>
                        <div className="flex justify-between"><span className="text-muted-foreground">上游错误率</span><b>{total > 0 ? ((upstreamErrCount / total) * 100).toFixed(2) : '0.00'}%</b></div>
                      </div>
                    </div>
                  </div>
                );
              })()}
            </CardContent>
          </Card>

          {/* Traffic / Token Trend */}
          <Card className="card-hover">
            <CardHeader className="flex flex-row items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold leading-none">{t('dash.trafficTokenTrend')}</h2>
                <CardDescription>{t('dash.trafficTokenTrendSub')}</CardDescription>
              </div>
              <div className="flex rounded-lg bg-muted/60 p-0.5">
                {CHART_OPTS.map(o => (
                  <button key={o} type="button" onClick={() => setChartOpt(o)}
                    className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${o === chartOpt ? 'bg-card text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}
                  >{o === 'token' ? 'Token' : o === '请求' ? t('usage.requests') : t('dash.errorRate')}</button>
                ))}
              </div>
            </CardHeader>
            <CardContent>
              {usageAggregateLoading ? (
                <div className="h-[285px] animate-pulse rounded-lg border bg-muted/20" />
              ) : chartData.length > 0 ? (
                <ResponsiveContainer width="100%" height={285}>
                  <AreaChart data={chartData} margin={{ left: -12, right: 8, top: 4 }}>
                    <defs><linearGradient id="tf" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.3} /><stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} /></linearGradient></defs>
                    <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                    <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} dy={6} />
                    <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
                    <Tooltip content={<DashboardChartTooltip />} />
                    <Area type="monotone" dataKey={chartOpt === 'token' ? 'total_tokens' : chartOpt === '请求' ? 'requests' : 'errors'} stroke="var(--chart-1)" strokeWidth={2.5} fill="url(#tf)" dot={{ r: 3 }} activeDot={{ r: 5 }} />
                  </AreaChart>
                </ResponsiveContainer>
              ) : (
                <p className="py-16 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
              )}
            </CardContent>
          </Card>

          {/* Request Logs */}
          <Card className="card-hover">
            <CardHeader className="flex flex-row items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold leading-none">{t('dash.requestLogs')}</h2>
                <CardDescription>{t('dash.requestLogsSub')}</CardDescription>
              </div>
              <Button variant="ghost" size="sm" onClick={() => window.location.href = '/usage'}>{t('dash.viewAllUsage')}</Button>
            </CardHeader>
            <CardContent className="p-0">
              {!recent ? (
                <div className="space-y-3 p-5">
                  {Array.from({ length: 6 }).map((_, i) => <div key={i} className="h-10 animate-pulse rounded bg-muted/60" />)}
                </div>
              ) : recent?.records.length === 0 ? (
                <p className="py-12 text-center text-sm text-muted-foreground">{t('dash.noRecentUsage')}</p>
              ) : (
                <div className="overflow-auto">
                  <table className="min-w-full border-collapse text-sm">
                    <thead>
                      <tr className="border-b bg-muted/20 text-left text-xs text-muted-foreground">
                        <th className="px-4 py-3 font-medium">{t('table.time')}</th>
                        <th className="px-4 py-3 font-medium">{t('table.status')}</th>
                        <th className="px-4 py-3 font-medium">{t('table.model')}</th>
                        <th className="px-4 py-3 font-medium">ID</th>
                        <th className="px-4 py-3 font-medium">{t('table.tokens')}</th>
                        <th className="px-4 py-3 font-medium">{t('table.latency')}</th>
                        <th className="px-4 py-3 font-medium">{t('table.key')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {recent?.records.slice(0, 8).map(r => (
                        <tr key={r.request_id} className="border-b last:border-0">
                          <td className="px-4 py-3 text-muted-foreground whitespace-nowrap">{new Date(r.timestamp).toLocaleString()}</td>
                          <td className="px-4 py-3">
                            <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${r.success ? 'bg-emerald-500/10 text-emerald-700' : 'bg-red-500/10 text-red-700'}`}>
                              <span className={`size-1.5 rounded-full ${r.success ? 'bg-emerald-500' : 'bg-red-500'}`} aria-hidden="true" />
                              {r.success ? t('usage.success') : t('usage.failure')}
                            </span>
                          </td>
                          <td className="px-4 py-3 font-medium text-foreground">{r.model}</td>
                          <td className="px-4 py-3 font-mono text-xs text-muted-foreground">{r.request_id}</td>
                          <td className="px-4 py-3">{r.total_tokens.toLocaleString()}</td>
                          <td className="px-4 py-3">{r.latency_ms}ms</td>
                          <td className="px-4 py-3 font-mono text-xs text-muted-foreground">{r.api_key_name ?? '—'}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="border-t px-4 py-2.5 text-[11px] text-muted-foreground">{t('dash.logsFooter', { count: Math.min(recent?.records.length ?? 0, 8) })}</div>
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* ── Right Column ── */}
        <div className="space-y-4">
          {/* Capacity & Budget */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="text-base font-semibold leading-none">{t('dash.capacityBudget')}</h2>
              <CardDescription>{t('dash.capacityBudgetSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-3">
                {[
                  { label: t('wallet.currentBalance'), val: fmt(sym, wo?.balance) },
                  { label: t('wallet.estimatedDays'), val: ed?.days != null ? `${ed.days.toFixed(1)}d` : '—' },
                  { label: t('dash.totalRequests'), val: agg?.total_requests.toLocaleString() ?? '—' },
                  { label: t('dash.totalCost'), val: fmt(sym, agg?.total_cost) },
                ].map(m => (
                  <div key={m.label} className="rounded-lg border bg-muted/20 p-3">
                    <div className="text-xs text-muted-foreground">{m.label}</div>
                    <div className="mt-1.5 text-lg font-semibold tracking-tight">{m.val}</div>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>

          {/* Model Distribution */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="text-base font-semibold leading-none">{t('dash.modelDistribution')}</h2>
              <CardDescription>{t('dash.modelDistributionSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              {modelActivityLoading ? (
                <div className="h-[140px] animate-pulse rounded-lg border bg-muted/20" />
              ) : modelShare.length > 0 ? (
                <div className="grid grid-cols-[140px_1fr] items-center gap-4">
                  <ResponsiveContainer width="100%" height={140}>
                    <PieChart>
                      <Pie data={modelShare} dataKey="count" nameKey="model" innerRadius={44} outerRadius={66} paddingAngle={2} strokeWidth={0}>
                        {modelShare.map((e, i) => <Cell key={e.model} fill={CHART_COLORS[i % CHART_COLORS.length]} />)}
                      </Pie>
                      <Tooltip content={<DashboardChartTooltip />} />
                    </PieChart>
                  </ResponsiveContainer>
                  <div className="space-y-2">
                    {modelShare.map((m, i) => (
                      <div key={m.model} className="flex items-center justify-between gap-2 text-xs">
                        <span className="flex items-center gap-1.5 truncate">
                          <span className="size-2 shrink-0 rounded-full" style={{ background: CHART_COLORS[i % CHART_COLORS.length] }} />
                          <span className="truncate text-muted-foreground">{m.model}</span>
                        </span>
                        <span className="shrink-0 font-medium">{m.percentage.toFixed(1)}%</span>
                      </div>
                    ))}
                  </div>
                </div>
              ) : (
                <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
              )}
            </CardContent>
          </Card>

          {/* Risk Alerts */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="text-base font-semibold leading-none">{t('dash.riskAlerts')}</h2>
              <CardDescription>{t('dash.riskAlertsSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              {alerts.length === 0 ? (
                <p className="py-6 text-center text-sm text-muted-foreground">{t('dash.noAlerts')}</p>
              ) : (
                <div className="space-y-2">
                  {alerts.map(a => (
                    <div key={a.id} className={`flex gap-3 rounded-lg border p-3 ${a.warn ? 'bg-amber-500/5 border-amber-200/40' : 'bg-muted/20'}`}>
                      <div className={`mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md ${a.warn ? 'bg-amber-500/15 text-amber-700' : 'bg-brand/10 text-brand'}`}>
                        {a.warn ? <AlertTriangle className="size-3.5" /> : <Info className="size-3.5" />}
                      </div>
                      <div>
                        <div className="text-xs font-medium text-foreground">{a.title}</div>
                        <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">{a.desc}</p>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>

          {/* Announcements */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
                <Bell className="size-4" />
                {t('dash.announcements')}
              </h2>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">{t('dash.announcementsSub')}</p>
              {!announcements || announcements.length === 0 ? (
                <p className="mt-3 text-sm text-muted-foreground">{t('dash.noAnnouncements')}</p>
              ) : (
                <div className="mt-3 space-y-3">
                  {announcements.map((a) => (
                    <div key={a.id} className="border-l-2 border-brand pl-3">
                      <p className="text-sm font-medium">{a.title}</p>
                      <p className="text-xs text-muted-foreground mt-0.5 whitespace-pre-wrap line-clamp-3">{a.content}</p>
                      <p className="text-[10px] text-muted-foreground/60 mt-1">{new Date(a.created_at).toLocaleDateString()}</p>
                    </div>
                  ))}
                </div>
              )}
            </CardContent>
          </Card>

          {/* FAQ */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
                <HelpCircle className="size-4" />
                {t('dash.faq')}
              </h2>
            </CardHeader>
            <CardContent>
              <p className="text-xs text-muted-foreground">{t('dash.faqSub')}</p>
              <p className="mt-3 text-sm text-muted-foreground">{t('dash.noFaq')}</p>
            </CardContent>
          </Card>
        </div>

      </section>

    </div>
  );
}
