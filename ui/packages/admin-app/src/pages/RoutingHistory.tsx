import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { usePublicModels } from '@fluxeme/shared/src/api/models';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { fetchRoutingHistory } from '@fluxeme/shared/src/api/routing';
import type {
  RoutingHistoryChannelSeries,
  RoutingHistoryResponse,
  RoutingHistorySummary,
} from '@fluxeme/shared/src/api/routing';

const CHANNEL_COLORS = [
  'var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)',
  'var(--chart-5)', 'var(--sidebar-primary)', 'var(--destructive)',
];

type Preset = '1h' | '24h' | '7d' | '30d' | 'custom';
type ChartPoint = { bucket: string; label: string; [channelId: string]: string | number | null };

function formatNumber(value: number | null | undefined): string {
  return value == null ? '—' : value.toLocaleString('zh-CN');
}

function formatPercent(value: number | null | undefined): string {
  return value == null || Number.isNaN(value) ? '—' : `${value.toFixed(1)}%`;
}

function formatMs(value: number | null | undefined): string {
  return value == null || Number.isNaN(value) ? '—' : `${Math.round(value)} ms`;
}

function toApiDate(value: string): string {
  if (!value) return '';
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? '' : parsed.toISOString();
}

function presetRange(preset: Exclude<Preset, 'custom'>): { start: string; end: string } {
  const end = new Date();
  const duration = { '1h': 3600000, '24h': 86400000, '7d': 7 * 86400000, '30d': 30 * 86400000 }[preset];
  const start = new Date(end.getTime() - duration);
  return { start: start.toISOString(), end: end.toISOString() };
}

function formatBucket(bucket: string, unit: RoutingHistoryResponse['bucket_unit']): string {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) return bucket;
  return unit === 'day'
    ? date.toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', timeZone: 'UTC' })
    : date.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', timeZone: 'UTC' });
}

function getChannelName(id: string, names: Map<string, string>, series?: RoutingHistoryChannelSeries): string {
  return names.get(id) || series?.channel_name || id;
}

function chartData(response: RoutingHistoryResponse, success: boolean): ChartPoint[] {
  return response.buckets.map((bucket, index) => {
    const point: ChartPoint = { bucket, label: formatBucket(bucket, response.bucket_unit) };
    Object.entries(response.series).forEach(([channelId, series]) => {
      point[channelId] = success ? series.success_rate_percent[index] ?? null : series.requests[index] ?? 0;
    });
    return point;
  });
}

function SummaryRow({ row, names, total }: { row: RoutingHistorySummary; names: Map<string, string>; total: number }) {
  const [expanded, setExpanded] = useState(false);
  return (
    <>
      <tr className="border-b border-border bg-muted/30">
        <td className="px-4 py-3">
          <button type="button" className="flex items-center gap-2 text-left font-medium text-foreground hover:text-primary" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded}>
            <span className="w-4 text-muted-foreground">{expanded ? '−' : '+'}</span>{getChannelName(row.channel_id, names)}
          </button>
          <div className="ml-6 mt-1 font-mono text-[10px] text-muted-foreground">{row.channel_id}</div>
        </td>
        <td className="px-4 py-3 text-right tabular-nums">{total > 0 ? `${((row.requests / total) * 100).toFixed(1)}%` : '—'}</td>
        <td className="px-4 py-3 text-right tabular-nums">{formatNumber(row.requests)}</td>
        <td className="px-4 py-3 text-right tabular-nums">{formatPercent(row.success_rate_percent)}</td>
        <td className="px-4 py-3 text-right tabular-nums">{formatMs(row.avg_latency_ms)}</td>
        <td className="px-4 py-3 text-right tabular-nums">{formatMs(row.p95_latency_ms)}</td>
      </tr>
      {expanded && row.endpoints.map((endpoint, index) => (
        <tr key={`${row.channel_id}-${endpoint.endpoint_id ?? 'unknown'}-${index}`} className="border-b border-border/60 text-sm">
          <td className="py-3 pl-12 pr-4 text-muted-foreground">
            <div>{endpoint.url || '未识别端点'}</div>
            <div className="mt-1 text-[10px]">{endpoint.endpoint_id == null ? '渠道级 / 未识别端点' : `Endpoint #${endpoint.endpoint_id}`}{endpoint.url_status === 'varied' ? ` · ${endpoint.url_variant_count} 个 URL 变体` : ''}</div>
          </td>
          <td className="px-4 py-3 text-right text-muted-foreground">{total > 0 ? `${((endpoint.requests / total) * 100).toFixed(1)}%` : '—'}</td>
          <td className="px-4 py-3 text-right tabular-nums">{formatNumber(endpoint.requests)}</td>
          <td className="px-4 py-3 text-right tabular-nums">{formatPercent(endpoint.success_rate_percent)}</td>
          <td className="px-4 py-3 text-right tabular-nums">{formatMs(endpoint.avg_latency_ms)}</td>
          <td className="px-4 py-3 text-right tabular-nums">{formatMs(endpoint.p95_latency_ms)}</td>
        </tr>
      ))}
    </>
  );
}

export default function RoutingHistory() {
  const { t } = useTranslation();
  const { data: models } = usePublicModels();
  const { data: channels } = useChannels();
  const [preset, setPreset] = useState<Preset>('24h');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [model, setModel] = useState('all');
  const [appliedRange, setAppliedRange] = useState(() => presetRange('24h'));
  const [data, setData] = useState<RoutingHistoryResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const requestVersion = useRef(0);

  const channelNames = useMemo(() => new Map((channels ?? []).map((channel) => [channel.id, channel.name || channel.id])), [channels]);
  const load = useCallback(async (range: { start: string; end: string }) => {
    const version = ++requestVersion.current;
    setLoading(true);
    setError(false);
    try {
      const response = await fetchRoutingHistory(toApiDate(range.start), toApiDate(range.end), model === 'all' ? undefined : model);
      if (version === requestVersion.current) setData(response);
    } catch {
      if (version === requestVersion.current) setError(true);
    } finally {
      if (version === requestVersion.current) setLoading(false);
    }
  }, [model]);

  useEffect(() => { void load(appliedRange); }, [appliedRange, load]);

  const applyPreset = (next: Exclude<Preset, 'custom'>) => { setPreset(next); setAppliedRange(presetRange(next)); };
  const applyCustom = () => {
    if (!customStart || !customEnd || new Date(customStart) >= new Date(customEnd)) return;
    setPreset('custom');
    setAppliedRange({ start: customStart, end: customEnd });
  };

  const volumeData = useMemo(() => data ? chartData(data, false) : [], [data]);
  const successData = useMemo(() => data ? chartData(data, true) : [], [data]);
  const channelIds = useMemo(() => data ? Object.keys(data.series).sort() : [], [data]);
  const total = data?.totals.requests ?? 0;
  const rows = useMemo(() => data ? [...data.summary].sort((a, b) => b.requests - a.requests) : [], [data]);
  const presets: Array<{ key: Exclude<Preset, 'custom'>; label: string }> = [
    { key: '1h', label: t('routingFlow.history1h') }, { key: '24h', label: t('routingFlow.history24h') },
    { key: '7d', label: t('routingFlow.history7d') }, { key: '30d', label: t('routingFlow.history30d') },
  ];

  return (
    <div className="space-y-5 animate-fade-in">
      <header><h1 className="text-2xl font-semibold tracking-tight">历史负载查询</h1><p className="mt-1 text-sm text-muted-foreground">查看已发布模型在各路由渠道的请求量、成功率和延迟表现。</p></header>

      <section className="rounded-xl border border-border bg-card p-4 shadow-sm" aria-label="历史查询筛选条件">
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex rounded-lg bg-muted p-1" role="group" aria-label="快捷时间范围">
            {presets.map((item) => <button key={item.key} type="button" onClick={() => applyPreset(item.key)} className={`rounded-md px-3 py-1.5 text-xs transition ${preset === item.key ? 'bg-background font-semibold text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}>{item.label}</button>)}
          </div>
          <label className="space-y-1 text-xs text-muted-foreground"><span>开始时间（本地输入，按 UTC 查询）</span><input aria-label="开始时间" type="datetime-local" value={customStart} onChange={(event) => setCustomStart(event.target.value)} className="block h-9 rounded-md border border-border bg-background px-2 text-xs text-foreground" /></label>
          <label className="space-y-1 text-xs text-muted-foreground"><span>结束时间</span><input aria-label="结束时间" type="datetime-local" value={customEnd} onChange={(event) => setCustomEnd(event.target.value)} className="block h-9 rounded-md border border-border bg-background px-2 text-xs text-foreground" /></label>
          <button type="button" onClick={applyCustom} disabled={!customStart || !customEnd} className="h-9 rounded-md bg-primary px-4 text-xs font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50">应用</button>
          <label className="ml-auto space-y-1 text-xs text-muted-foreground"><span>模型</span><select aria-label="模型筛选" value={model} onChange={(event) => setModel(event.target.value)} className="block h-9 min-w-44 rounded-md border border-border bg-background px-2 text-xs text-foreground"><option value="all">全部已发布模型</option>{(models ?? []).map((item) => <option key={item.id} value={item.name}>{item.name}</option>)}</select></label>
        </div>
        <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-muted-foreground"><span className="rounded-full bg-muted px-2.5 py-1">数据源：ClickHouse usage_events</span><span className="rounded-full bg-muted px-2.5 py-1">时区：UTC</span>{data ? <span className="rounded-full bg-muted px-2.5 py-1">粒度：{data.bucket_unit === 'day' ? '按天' : '按小时'}</span> : null}</div>
      </section>

      {loading ? <div className="rounded-xl border border-dashed border-border p-12 text-center text-sm text-muted-foreground">正在查询历史负载…</div> : error ? <div className="rounded-xl border border-dashed border-destructive/50 bg-destructive/5 p-10 text-center"><p className="text-sm text-destructive">历史负载查询失败</p><button type="button" onClick={() => void load(appliedRange)} className="mt-3 rounded-md border border-border bg-background px-3 py-1.5 text-xs hover:bg-muted">重试</button></div> : !data || data.totals.requests === 0 ? <div className="rounded-xl border border-dashed border-border p-12 text-center text-sm text-muted-foreground">当前时间范围没有观测数据</div> : (
        <>
          <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">{[
            ['总请求数', formatNumber(data.totals.requests), '观测到的请求总量'], ['成功率', formatPercent(data.totals.success_rate_percent), '成功请求 / 总请求'],
            ['平均延迟', formatMs(data.totals.avg_latency_ms), '按请求数加权'], ['P95 延迟', formatMs(data.totals.p95_latency_ms), '全渠道请求分位数'],
            ['未识别端点', formatNumber(data.totals.unattributed_requests), '保留原始归属，不做均摊'],
          ].map(([label, value, note]) => <div key={label} className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="text-xs text-muted-foreground">{label}</div><div className="mt-2 text-2xl font-semibold tracking-tight tabular-nums">{value}</div><div className="mt-1 text-[10px] text-muted-foreground">{note}</div></div>)}</section>

          <div className="grid gap-4 xl:grid-cols-2">
            <section className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="mb-4"><h2 className="font-semibold">请求量趋势</h2><p className="text-xs text-muted-foreground">按渠道统计请求数 · 空 bucket 表示 0 请求</p></div><div className="h-72"><ResponsiveContainer width="100%" height="100%"><BarChart data={volumeData} margin={{ left: 0, right: 12, top: 8, bottom: 8 }}><CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} /><XAxis dataKey="label" tick={{ fontSize: 10 }} minTickGap={24} /><YAxis allowDecimals={false} tick={{ fontSize: 10 }} /><Tooltip formatter={(value, name) => [formatNumber(Number(value)), getChannelName(String(name), channelNames, data.series[String(name)])]} labelFormatter={(label) => `时间：${label}`} /><Legend formatter={(value) => getChannelName(String(value), channelNames, data.series[String(value)])} />{channelIds.map((id, index) => <Bar key={id} dataKey={id} name={id} stackId="requests" fill={CHANNEL_COLORS[index % CHANNEL_COLORS.length]} />)}</BarChart></ResponsiveContainer></div></section>
            <section className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="mb-4"><h2 className="font-semibold">成功率趋势</h2><p className="text-xs text-muted-foreground">按渠道统计 · 无请求 bucket 不显示为 0%</p></div><div className="h-72"><ResponsiveContainer width="100%" height="100%"><LineChart data={successData} margin={{ left: 0, right: 12, top: 8, bottom: 8 }}><CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} /><XAxis dataKey="label" tick={{ fontSize: 10 }} minTickGap={24} /><YAxis domain={[0, 100]} tick={{ fontSize: 10 }} tickFormatter={(value) => `${value}%`} /><Tooltip formatter={(value, name) => [value == null ? '—' : `${Number(value).toFixed(1)}%`, getChannelName(String(name), channelNames, data.series[String(name)])]} labelFormatter={(label) => `时间：${label}`} /><Legend formatter={(value) => getChannelName(String(value), channelNames, data.series[String(value)])} />{channelIds.map((id, index) => <Line key={id} dataKey={id} name={id} connectNulls={false} stroke={CHANNEL_COLORS[index % CHANNEL_COLORS.length]} strokeWidth={2} dot={false} />)}</LineChart></ResponsiveContainer></div></section>
          </div>

          <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm"><div className="border-b border-border px-4 py-4"><h2 className="font-semibold">渠道与端点明细</h2><p className="mt-1 text-xs text-muted-foreground">渠道父行使用渠道级聚合；点击展开查看真实端点归属。未识别端点不会被分摊。</p></div><div className="overflow-x-auto"><table className="w-full min-w-[820px] border-collapse text-sm"><caption className="sr-only">历史负载渠道与端点统计</caption><thead><tr className="border-b border-border bg-muted/20 text-xs text-muted-foreground"><th className="px-4 py-3 text-left">渠道 / 端点</th><th className="px-4 py-3 text-right">请求占比</th><th className="px-4 py-3 text-right">请求数</th><th className="px-4 py-3 text-right">成功率</th><th className="px-4 py-3 text-right">平均延迟</th><th className="px-4 py-3 text-right">P95 延迟</th></tr></thead><tbody>{rows.map((row) => <SummaryRow key={row.channel_id} row={row} names={channelNames} total={total} />)}</tbody></table></div></section>
        </>
      )}
    </div>
  );
}
