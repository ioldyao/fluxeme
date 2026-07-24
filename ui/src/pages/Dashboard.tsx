import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Activity, AlertTriangle, Bell, HelpCircle, Info, Layers3, ShieldCheck, Wallet,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';
import { useCurrency, CURRENCY_SYMBOL } from '@/store/currency';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useUsage, useUsageAggregate, useModelActivity } from '@/api/usage';
import { useEstimatedDays, useWalletOverview } from '@/api/wallet';
import { useRoutingHistory } from '@/api/routing';
import { DashboardChartTooltip } from '@/components/dashboard/DashboardChartTooltip';
import {
  Area, AreaChart, CartesianGrid, Cell, Pie, PieChart,
  ResponsiveContainer, Tooltip, XAxis, YAxis,
} from 'recharts';

const RANGE_DAYS = [1, 7, 30] as const;
const CHART_COLORS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'];
const CHART_OPTS = ['token', '请求', '错误率'] as const;

function fmt(sym: string, r: number, v?: number) {
  const a = v ?? 0;
  return `${sym}${(sym === '¥' ? a * r : a).toFixed(2)}`;
}

function fmtLat(ms: number) {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms.toFixed(0)}ms`;
}

export default function Dashboard() {
  const { t } = useTranslation();
  const [days, setDays] = useState(7);
  const [chartOpt, setChartOpt] = useState<string>(CHART_OPTS[0]);

  const { data: stats, refetch } = useDashboard();
  const { data: agg, refetch: ra } = useDashboardAggregations();
  const { data: ua, refetch: rua } = useUsageAggregate(days);
  const { data: ma, refetch: rma } = useModelActivity(days);
  const { data: recent, refetch: rrl } = useUsage({ limit: 8 });
  const { data: wo, refetch: rwo } = useWalletOverview();
  const { data: ed, refetch: red } = useEstimatedDays();
  const { data: rh, refetch: rrh } = useRoutingHistory(days);
  const { currency, rate } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];

  const availability = agg?.success_rate_24h ?? 0;
  const avgLat = agg?.avg_latency_ms_24h ?? 0;
  const modelCount = stats?.models ?? 0;
  const apiKeyCount = stats?.api_keys ?? 0;
  const requests24h = agg?.requests_24h ?? 0;
  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const toneCls = availability >= 99 ? 'bg-emerald-500 shadow-[0_0_0_6px_rgba(20,150,106,0.12)]' : availability >= 95 ? 'bg-amber-500 shadow-[0_0_0_6px_rgba(217,145,19,0.14)]' : 'bg-red-500 shadow-[0_0_0_6px_rgba(216,75,75,0.14)]';
  const toneLabel = availability >= 99 ? t('gateway.healthy') : availability >= 95 ? t('gateway.degraded') : t('gateway.unstable');

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

  // routing rows
  const routingRows = useMemo(() => {
    if (!rh?.summary) return [];
    const total = rh.summary.reduce((s, r) => s + r.requests, 0);
    return rh.summary.slice().sort((a, b) => b.requests - a.requests).slice(0, 3).map(r => ({
      name: rh.series[r.channel_id]?.channel_name ?? r.channel_id,
      share: total > 0 ? (r.requests / total) * 100 : 0,
      requests: r.requests,
      latency: r.avg_latency,
      rate: r.success_rate,
    }));
  }, [rh]);

  // alerts
  const alerts = useMemo(() => {
    const a: { id: string; title: string; desc: string; warn: boolean }[] = [];
    if (avgLat > 2000) a.push({ id: 'lat', title: t('dash.alertLatencyTitle'), desc: t('dash.alertLatencyDesc', { latency: avgLat.toFixed(0) }), warn: true });
    if (availability < 95) a.push({ id: 'suc', title: t('dash.alertSuccessTitle'), desc: t('dash.alertSuccessDesc', { rate: availability.toFixed(1) }), warn: true });
    if ((modelShare[0]?.percentage ?? 0) > 80) a.push({ id: 'con', title: t('dash.alertConcentrationTitle'), desc: t('dash.alertConcentrationDesc', { model: modelShare[0]?.model ?? '—', share: (modelShare[0]?.percentage ?? 0).toFixed(1) }), warn: false });
    if ((ed?.days ?? Infinity) < 10) a.push({ id: 'bal', title: t('dash.alertBalanceTitle'), desc: t('dash.alertBalanceDesc', { days: (ed?.days ?? 0).toFixed(1) }), warn: true });
    return a;
  }, [avgLat, availability, modelShare, ed?.days, t]);

  const handleRefresh = () => { void refetch(); void ra(); void rua(); void rma(); void rrl(); void rwo(); void red(); void rrh(); };

  const chartData = useMemo(() => {
    if (!ua?.length) return [];
    return ua.map(d => ({ date: d.date.slice(5), requests: d.count, total_tokens: d.total_tokens, errors: d.count - d.success_count }));
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
                <div className="mt-1 text-sm text-muted-foreground">{t('dash.gatewayHealthMeta', { modelCount, channelCount: stats?.channels ?? 0, apiKeyCount })}</div>
              </div>
            </div>
            <div className="text-right">
              <div className="text-2xl font-semibold tracking-tight">{availability.toFixed(2)}%</div>
              <div className="text-xs text-muted-foreground">{t('dash.availability')}</div>
            </div>
          </div>
        </div>
        {[
          { title: t('dash.requests'), val: requests24h.toLocaleString(), hint: t('dash.last24Hours'), icon: <Activity className="size-4" /> },
          { title: t('usage.totalTokens'), val: totalTokens24h.toLocaleString(), hint: t('dash.last24Hours'), icon: <Layers3 className="size-4" /> },
          { title: t('dash.avgLatency'), val: fmtLat(avgLat), hint: t('dash.performanceSub'), icon: <ShieldCheck className="size-4" /> },
          { title: t('dash.cost24h'), val: fmt(sym, rate, agg?.cost_24h), hint: t('dash.last24Hours'), icon: <Wallet className="size-4" /> },
        ].map(m => (
          <div key={m.title} className="rounded-xl border bg-card p-4 shadow-sm">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span className="rounded-md bg-brand/10 p-1 text-brand">{m.icon}</span> {m.title}
            </div>
            <div className="mt-3 text-2xl font-semibold tracking-tight">{m.val}</div>
            <div className="mt-1 text-xs text-muted-foreground">{m.hint}</div>
          </div>
        ))}
      </section>

      {/* Dashboard Grid: left main + right rail */}
      <section className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.75fr)_minmax(310px,0.8fr)]" style={{ alignItems: 'start' }}>

        {/* ── Left Column ── */}
        <div className="space-y-4">
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
              {chartData.length > 0 ? (
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

          {/* Request Flow */}
          <Card className="card-hover">
            <CardHeader>
              <h2 className="text-base font-semibold leading-none">{t('dash.requestFlow')}</h2>
              <CardDescription>{t('dash.requestFlowSub')}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_auto_1fr_auto_1fr] xl:items-center">
                {[
                  { title: t('dash.requestIngress'), sub: t('dash.requestIngressSub'), val: requests24h },
                  null,
                  { title: t('dash.gatewayProcessing'), sub: t('dash.gatewayProcessingSub'), val: requests24h },
                  null,
                  { title: t('dash.modelResponses'), sub: t('dash.modelResponsesSub'), val: Math.round(requests24h * (availability / 100)) },
                ].map((n, i) => n === null ? (
                  <div key={i} className="hidden justify-center text-muted-foreground xl:flex"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M5 12h14M13 5l7 7-7 7"/></svg></div>
                ) : (
                  <div key={i} className="rounded-lg border bg-muted/20 p-4">
                    <div className="text-sm font-medium text-foreground">{n.title}</div>
                    <div className="mt-0.5 text-xs text-muted-foreground">{n.sub}</div>
                    <div className="mt-4 text-2xl font-semibold tracking-tight">{n.val.toLocaleString()}</div>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>

          {/* Model Routing Performance */}
          <Card className="card-hover">
            <CardHeader className="flex flex-row items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold leading-none">{t('dash.routingPerformance')}</h2>
                <CardDescription>{t('dash.routingPerformanceSub')}</CardDescription>
              </div>
              <Button variant="ghost" size="sm" onClick={() => window.location.href = '/routing-history'}>{t('dash.viewRouting')}</Button>
            </CardHeader>
            <CardContent>
              {routingRows.length === 0 ? (
                <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
              ) : (
                <div className="space-y-1">
                  {routingRows.map((r, i) => (
                    <div key={r.name} className="grid grid-cols-[minmax(0,1fr)_80px_80px] items-center gap-3 border-t border-border/60 px-0 py-3 text-sm first:border-0">
                      <div className="flex items-center gap-2.5 min-w-0">
                        <span className={`size-2 shrink-0 rounded-full ${i === 0 ? 'bg-brand' : i === 1 ? 'bg-blue-500' : 'bg-muted-foreground/40'}`} />
                        <span className="truncate font-medium text-foreground">{r.name}</span>
                      </div>
                      <div className="text-right">
                        <div className="truncate font-semibold">{r.share.toFixed(1)}%</div>
                        <div className="text-[11px] text-muted-foreground">{r.requests.toLocaleString()}</div>
                      </div>
                      <div className="text-right text-muted-foreground">{fmtLat(r.latency)}</div>
                    </div>
                  ))}
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
                  { label: t('wallet.currentBalance'), val: fmt(sym, rate, wo?.balance) },
                  { label: t('wallet.estimatedDays'), val: ed?.days != null ? `${ed.days.toFixed(1)}d` : '—' },
                  { label: t('dash.totalRequests'), val: agg?.total_requests.toLocaleString() ?? '—' },
                  { label: t('dash.totalCost'), val: fmt(sym, rate, agg?.total_cost) },
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
              {modelShare.length > 0 ? (
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
              <p className="mt-3 text-sm text-muted-foreground">{t('dash.noAnnouncements')}</p>
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

        {/* ── Request Logs (full width at bottom) ── */}
        <Card className="card-hover xl:col-span-2">
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
      </section>
    </div>
  );
}
