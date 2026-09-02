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

function formatNumber(value: number | null | undefined, locale = 'zh-CN'): string {
  return value == null ? '—' : value.toLocaleString(locale);
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

function formatBucket(bucket: string, unit: RoutingHistoryResponse['bucket_unit'], locale = 'zh-CN'): string {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) return bucket;
  return unit === 'day'
    ? date.toLocaleDateString(locale, { month: '2-digit', day: '2-digit', timeZone: 'UTC' })
    : date.toLocaleString(locale, { month: '2-digit', day: '2-digit', hour: '2-digit', timeZone: 'UTC' });
}

function getChannelName(id: string, names: Map<string, string>, series?: RoutingHistoryChannelSeries): string {
  return names.get(id) || series?.channel_name || id;
}

function chartData(response: RoutingHistoryResponse, success: boolean, locale = 'zh-CN'): ChartPoint[] {
  return response.buckets.map((bucket, index) => {
    const point: ChartPoint = { bucket, label: formatBucket(bucket, response.bucket_unit, locale) };
    Object.entries(response.series).forEach(([channelId, series]) => {
      point[channelId] = success ? series.success_rate_percent[index] ?? null : series.requests[index] ?? 0;
    });
    return point;
  });
}

function SummaryRow({ row, names, total, t }: { row: RoutingHistorySummary; names: Map<string, string>; total: number; t: (key: string, options?: Record<string, unknown>) => string }) {
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
            <div>{endpoint.url || t('routingHistory.endpointUnknown')}</div>
            <div className="mt-1 text-[10px]">{endpoint.endpoint_id == null ? t('routingHistory.channelUnknownEndpoint') : `Endpoint #${endpoint.endpoint_id}`}{endpoint.url_status === 'varied' ? ` · ${t('routingHistory.urlVariants', { count: endpoint.url_variant_count })}` : ''}</div>
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
  const { t, i18n } = useTranslation();
  const locale = i18n.language.startsWith('zh') ? 'zh-CN' : 'en-US';
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

  const volumeData = useMemo(() => data ? chartData(data, false, locale) : [], [data, locale]);
  const successData = useMemo(() => data ? chartData(data, true, locale) : [], [data, locale]);
  const channelIds = useMemo(() => data ? Object.keys(data.series).sort() : [], [data]);
  const total = data?.totals.requests ?? 0;
  const rows = useMemo(() => data ? [...data.summary].sort((a, b) => b.requests - a.requests) : [], [data]);
  const presets: Array<{ key: Exclude<Preset, 'custom'>; label: string }> = [
    { key: '1h', label: t('routingFlow.history1h') }, { key: '24h', label: t('routingFlow.history24h') },
    { key: '7d', label: t('routingFlow.history7d') }, { key: '30d', label: t('routingFlow.history30d') },
  ];

  return (
    <div className="space-y-5 animate-fade-in">
      <header><h1 className="text-2xl font-semibold tracking-tight">{t('routingHistory.title')}</h1><p className="mt-1 text-sm text-muted-foreground">{t('routingHistory.subtitle')}</p></header>

      <section className="rounded-xl border border-border bg-card p-4 shadow-sm" aria-label={t('routingHistory.filters')}>
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex rounded-lg bg-muted p-1" role="group" aria-label={t('routingHistory.quickRanges')}>
            {presets.map((item) => <button key={item.key} type="button" onClick={() => applyPreset(item.key)} className={`rounded-md px-3 py-1.5 text-xs transition ${preset === item.key ? 'bg-background font-semibold text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'}`}>{item.label}</button>)}
          </div>
          <label className="space-y-1 text-xs text-muted-foreground"><span>{t('routingHistory.startTimeHint')}</span><input aria-label={t('routingHistory.startTime')} type="datetime-local" value={customStart} onChange={(event) => setCustomStart(event.target.value)} className="block h-9 rounded-md border border-border bg-background px-2 text-xs text-foreground" /></label>
          <label className="space-y-1 text-xs text-muted-foreground"><span>{t('routingHistory.endTime')}</span><input aria-label={t('routingHistory.endTime')} type="datetime-local" value={customEnd} onChange={(event) => setCustomEnd(event.target.value)} className="block h-9 rounded-md border border-border bg-background px-2 text-xs text-foreground" /></label>
          <button type="button" onClick={applyCustom} disabled={!customStart || !customEnd} className="h-9 rounded-md bg-primary px-4 text-xs font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50">{t('routingHistory.apply')}</button>
          <label className="ml-auto space-y-1 text-xs text-muted-foreground"><span>{t('routingHistory.model')}</span><select aria-label={t('routingHistory.modelFilter')} value={model} onChange={(event) => setModel(event.target.value)} className="block h-9 min-w-44 rounded-md border border-border bg-background px-2 text-xs text-foreground"><option value="all">{t('routingHistory.allPublishedModels')}</option>{(models ?? []).map((item) => <option key={item.id} value={item.name}>{item.name}</option>)}</select></label>
        </div>
        <div className="mt-3 flex flex-wrap gap-2 text-[11px] text-muted-foreground"><span className="rounded-full bg-muted px-2.5 py-1">{t('routingHistory.dataSource')}</span><span className="rounded-full bg-muted px-2.5 py-1">{t('routingHistory.timezone')}</span>{data ? <span className="rounded-full bg-muted px-2.5 py-1">{t('routingHistory.granularity', { value: data.bucket_unit === 'day' ? t('routingHistory.byDay') : t('routingHistory.byHour') })}</span> : null}</div>
      </section>

      {loading ? <div className="rounded-xl border border-dashed border-border p-12 text-center text-sm text-muted-foreground">{t('routingHistory.loading')}</div> : error ? <div className="rounded-xl border border-dashed border-destructive/50 bg-destructive/5 p-10 text-center"><p className="text-sm text-destructive">{t('routingHistory.queryFailed')}</p><button type="button" onClick={() => void load(appliedRange)} className="mt-3 rounded-md border border-border bg-background px-3 py-1.5 text-xs hover:bg-muted">{t('routingHistory.retry')}</button></div> : !data || data.totals.requests === 0 ? <div className="rounded-xl border border-dashed border-border p-12 text-center text-sm text-muted-foreground">{t('routingHistory.noData')}</div> : (
        <>
          <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">{[
            [t('routingHistory.totalRequests'), formatNumber(data.totals.requests), t('routingHistory.totalRequestsHint')], [t('routingHistory.successRate'), formatPercent(data.totals.success_rate_percent), t('routingHistory.successRateHint')],
            [t('routingHistory.avgLatency'), formatMs(data.totals.avg_latency_ms), t('routingHistory.avgLatencyHint')], [t('routingHistory.p95Latency'), formatMs(data.totals.p95_latency_ms), t('routingHistory.p95LatencyHint')],
            [t('routingHistory.unattributed'), formatNumber(data.totals.unattributed_requests), t('routingHistory.unattributedHint')],
          ].map(([label, value, note]) => <div key={label} className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="text-xs text-muted-foreground">{label}</div><div className="mt-2 text-2xl font-semibold tracking-tight tabular-nums">{value}</div><div className="mt-1 text-[10px] text-muted-foreground">{note}</div></div>)}</section>

          <div className="grid gap-4 xl:grid-cols-2">
            <section className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="mb-4"><h2 className="font-semibold">{t('routingHistory.volumeTitle')}</h2><p className="text-xs text-muted-foreground">{t('routingHistory.volumeSubtitle')}</p></div><div className="h-72"><ResponsiveContainer width="100%" height="100%"><BarChart data={volumeData} margin={{ left: 0, right: 12, top: 8, bottom: 8 }}><CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} /><XAxis dataKey="label" tick={{ fontSize: 10 }} minTickGap={24} /><YAxis allowDecimals={false} tick={{ fontSize: 10 }} /><Tooltip formatter={(value, name) => [formatNumber(Number(value)), getChannelName(String(name), channelNames, data.series[String(name)])]} labelFormatter={(label) => t('routingHistory.time', { value: label })} /><Legend formatter={(value) => getChannelName(String(value), channelNames, data.series[String(value)])} />{channelIds.map((id, index) => <Bar key={id} dataKey={id} name={id} stackId="requests" fill={CHANNEL_COLORS[index % CHANNEL_COLORS.length]} />)}</BarChart></ResponsiveContainer></div></section>
            <section className="rounded-xl border border-border bg-card p-4 shadow-sm"><div className="mb-4"><h2 className="font-semibold">{t('routingHistory.successTitle')}</h2><p className="text-xs text-muted-foreground">{t('routingHistory.successSubtitle')}</p></div><div className="h-72"><ResponsiveContainer width="100%" height="100%"><LineChart data={successData} margin={{ left: 0, right: 12, top: 8, bottom: 8 }}><CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} /><XAxis dataKey="label" tick={{ fontSize: 10 }} minTickGap={24} /><YAxis domain={[0, 100]} tick={{ fontSize: 10 }} tickFormatter={(value) => `${value}%`} /><Tooltip formatter={(value, name) => [value == null ? '—' : `${Number(value).toFixed(1)}%`, getChannelName(String(name), channelNames, data.series[String(name)])]} labelFormatter={(label) => t('routingHistory.time', { value: label })} /><Legend formatter={(value) => getChannelName(String(value), channelNames, data.series[String(value)])} />{channelIds.map((id, index) => <Line key={id} dataKey={id} name={id} connectNulls={false} stroke={CHANNEL_COLORS[index % CHANNEL_COLORS.length]} strokeWidth={2} dot={false} />)}</LineChart></ResponsiveContainer></div></section>
          </div>

          <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm"><div className="border-b border-border px-4 py-4"><h2 className="font-semibold">{t('routingHistory.detailsTitle')}</h2><p className="mt-1 text-xs text-muted-foreground">{t('routingHistory.detailsSubtitle')}</p></div><div className="overflow-x-auto"><table className="w-full min-w-[820px] border-collapse text-sm"><caption className="sr-only">{t('routingHistory.detailsCaption')}</caption><thead><tr className="border-b border-border bg-muted/20 text-xs text-muted-foreground"><th className="px-4 py-3 text-left">{t('routingHistory.channelEndpoint')}</th><th className="px-4 py-3 text-right">{t('routingHistory.requestShare')}</th><th className="px-4 py-3 text-right">{t('routingHistory.requestCount')}</th><th className="px-4 py-3 text-right">{t('routingHistory.successRate')}</th><th className="px-4 py-3 text-right">{t('routingHistory.avgLatency')}</th><th className="px-4 py-3 text-right">{t('routingHistory.p95Latency')}</th></tr></thead><tbody>{rows.map((row) => <SummaryRow key={row.channel_id} row={row} names={channelNames} total={total} t={t} />)}</tbody></table></div></section>
        </>
      )}
    </div>
  );
}
