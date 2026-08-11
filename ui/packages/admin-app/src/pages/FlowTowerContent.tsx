import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useFlowMetrics } from '@fluxeme/shared';
import { useModels } from '@fluxeme/shared/src/api/models';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { useRoutingHealth } from '@fluxeme/shared/src/api/routing';
import type { RoutingHealthModel } from '@fluxeme/shared/src/api/routing';
import type { Channel, FlowMetricsClientIp, FlowMetricsModelShare, FlowMetricsPercentiles, Model } from '@fluxeme/shared/src/types';

type RangeKey = '5m' | '15m' | '1h' | '6h' | '24h';
type FlowTabKey = 'flow' | 'endpoint' | 'compare';
type Tone = 'blue' | 'cyan' | 'green' | 'yellow' | 'red';

type MetricRow = {
  label: string;
  value: string;
};

type ModelHealthStatus = 'healthy' | 'degraded' | 'unavailable' | 'unknown';

type CatalogModel = {
  config: Model;
  health?: RoutingHealthModel;
  status: ModelHealthStatus;
  enabledEndpoints: number;
  observedEndpoints: number;
  availableEndpoints: number;
  routedRequests24h?: number;
  successRate24h?: number;
  averageLatency24h?: number;
  highestChannelP95?: number;
  brokenCircuitChannels: number;
};

const RANGE_OPTIONS: Array<{ key: RangeKey; short: string; long: string; label: string }> = [
  { key: '5m', short: '5M', long: '5 分钟', label: '5 分钟' },
  { key: '15m', short: '15M', long: '15 分钟', label: '15 分钟' },
  { key: '1h', short: '1H', long: '1 小时', label: '1 小时' },
  { key: '6h', short: '6H', long: '6 小时', label: '6 小时' },
  { key: '24h', short: '24H', long: '24 小时', label: '24 小时' },
];

const RANGE_MINUTES: Record<RangeKey, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
};

const FLOW_TABS: Array<{ key: FlowTabKey; label: string }> = [
  { key: 'flow', label: '请求流' },
  { key: 'endpoint', label: '端点状态' },
  { key: 'compare', label: '模型对比' },
];

function formatNumber(value: number | null | undefined) {
  if (value == null) return '—';
  return value.toLocaleString('zh-CN');
}

function formatPercent(value: number | null | undefined) {
  if (value == null || Number.isNaN(value)) return '—';
  return `${value.toFixed(2)}%`;
}

function formatPercentile(value: number | null | undefined) {
  if (value == null || Number.isNaN(value)) return '—';
  return formatNumber(Math.round(value));
}

function formatIpRows(rows: FlowMetricsClientIp[]) {
  const top = rows[0]?.requests ?? 0;
  return rows.slice(0, 8).map((row) => ({
    ...row,
    ratio: top > 0 ? Math.max(8, Math.round((row.requests / top) * 100)) : 0,
  }));
}

function formatTrendBucket(bucket: string, unit: 'minute' | 'hour') {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) {
    return bucket;
  }
  if (unit === 'minute') {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false });
  }
  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    hour12: false,
  }).replace(' ', ' ');
}

function formatRangeBounds(range: RangeKey, nowMs = Date.now()) {
  const end = new Date(nowMs);
  const start = new Date(end.getTime() - RANGE_MINUTES[range] * 60_000);
  return {
    start: start.toISOString(),
    end: end.toISOString(),
  };
}

function inferRangeLabel(start?: string, end?: string) {
  if (!start || !end) return '当前区间';
  const startDate = new Date(start);
  const endDate = new Date(end);
  if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) return '当前区间';
  const minutes = Math.round((endDate.getTime() - startDate.getTime()) / 60_000);
  const preset = Object.entries(RANGE_MINUTES).find(([, value]) => value === minutes)?.[0] as RangeKey | undefined;
  if (preset) {
    return RANGE_OPTIONS.find((option) => option.key === preset)?.long ?? '当前区间';
  }
  return `${startDate.toLocaleString('zh-CN', { hour12: false })} ~ ${endDate.toLocaleString('zh-CN', { hour12: false })}`;
}

function formatContextLength(value: number | null | undefined) {
  if (!value) return '—';
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return formatNumber(value);
}

function modelHealthPresentation(status: ModelHealthStatus) {
  switch (status) {
    case 'healthy':
      return { label: '可路由', dot: 'bg-[#16a36a]', badge: 'bg-[#eaf8f1] text-[#15865a]' };
    case 'degraded':
      return { label: '部分降级', dot: 'bg-[#d99a18]', badge: 'bg-[#fff5dc] text-[#ad7411]' };
    case 'unavailable':
      return { label: '不可用', dot: 'bg-[#e24f4f]', badge: 'bg-[#ffecec] text-[#c83333]' };
    case 'unknown':
      return { label: '健康未知', dot: 'bg-[#98a2b3]', badge: 'bg-[#f1f3f6] text-[#667085]' };
  }
}

function deriveCatalogModel(
  config: Model,
  channelById: Map<string, Channel>,
  channelConfigReady: boolean,
  health?: RoutingHealthModel,
): CatalogModel {
  const healthChannels = health?.channels ?? [];
  const channels = healthChannels.filter((channel) => channel.enabled);
  const endpointAvailability = new Map(
    channels.flatMap((channel) => channel.endpoints.map((endpoint) => [endpoint.endpoint_id, endpoint.available])),
  );
  const configuredEndpoints = config.channels.flatMap((binding) => {
    const channel = channelById.get(binding.channel_id);
    if (!channel?.enabled) return [];
    return channel.endpoints.filter((endpoint) => endpoint.enabled);
  });
  const enabledEndpoints = configuredEndpoints;
  const observedEnabledEndpoints = enabledEndpoints.filter((endpoint) => endpoint.id != null && endpointAvailability.has(endpoint.id));
  const availableEndpoints = observedEnabledEndpoints.filter((endpoint) => endpoint.id != null && endpointAvailability.get(endpoint.id) === true);
  const totalRequests = healthChannels.reduce((sum, channel) => sum + channel.requests, 0);
  const weightedSuccess = totalRequests > 0
    ? healthChannels.reduce((sum, channel) => sum + channel.requests * channel.success_rate, 0) / totalRequests
    : undefined;
  const weightedLatency = totalRequests > 0
    ? healthChannels.reduce((sum, channel) => sum + channel.requests * channel.avg_latency_ms, 0) / totalRequests
    : undefined;
  const p95Values = healthChannels.filter((channel) => channel.requests > 0).map((channel) => channel.p95_latency_ms);
  const brokenCircuitChannels = channels.filter((channel) => channel.requests > 0 && channel.circuit_enabled && !channel.circuit_ok).length;

  const status: ModelHealthStatus = !channelConfigReady
    ? 'unknown'
    : enabledEndpoints.length === 0
      ? 'unavailable'
      : brokenCircuitChannels > 0 && observedEnabledEndpoints.length === 0
        ? 'unavailable'
        : observedEnabledEndpoints.length === 0
          ? 'unknown'
          : availableEndpoints.length === 0
            ? 'unavailable'
            : availableEndpoints.length < enabledEndpoints.length || observedEnabledEndpoints.length < enabledEndpoints.length || brokenCircuitChannels > 0
              ? 'degraded'
              : 'healthy';

  return {
    config,
    health,
    status,
    enabledEndpoints: enabledEndpoints.length,
    observedEndpoints: observedEnabledEndpoints.length,
    availableEndpoints: availableEndpoints.length,
    routedRequests24h: health ? health.total_requests : undefined,
    successRate24h: weightedSuccess,
    averageLatency24h: weightedLatency,
    highestChannelP95: p95Values.length > 0 ? Math.max(...p95Values) : undefined,
    brokenCircuitChannels,
  };
}

function toneClasses(tone: Tone) {
  switch (tone) {
    case 'blue':
      return {
        card: 'border-[#dce7ff] bg-[linear-gradient(180deg,#ffffff_0%,#f6f9ff_100%)]',
        badge: 'bg-[#edf4ff] text-[#3276e8]',
        value: 'text-[#3276e8]',
        glow: 'bg-[#edf4ff]',
      };
    case 'cyan':
      return {
        card: 'border-[#d8f0f5] bg-[linear-gradient(180deg,#ffffff_0%,#f3fcfe_100%)]',
        badge: 'bg-[#eafafb] text-[#0ca8bd]',
        value: 'text-[#0ca8bd]',
        glow: 'bg-[#eafafb]',
      };
    case 'green':
      return {
        card: 'border-[#d7f0e3] bg-[linear-gradient(180deg,#ffffff_0%,#f5fcf8_100%)]',
        badge: 'bg-[#ecfbf4] text-[#16a36a]',
        value: 'text-[#16a36a]',
        glow: 'bg-[#ecfbf4]',
      };
    case 'yellow':
      return {
        card: 'border-[#f3e4bc] bg-[linear-gradient(180deg,#ffffff_0%,#fffaf0_100%)]',
        badge: 'bg-[#fff8e8] text-[#d99a18]',
        value: 'text-[#d99a18]',
        glow: 'bg-[#fff8e8]',
      };
    case 'red':
      return {
        card: 'border-[#f2d8d8] bg-[linear-gradient(180deg,#ffffff_0%,#fff6f6_100%)]',
        badge: 'bg-[#fff0f0] text-[#e24f4f]',
        value: 'text-[#e24f4f]',
        glow: 'bg-[#fff0f0]',
      };
  }
}

function Panel({
  title,
  subtitle,
  right,
  children,
  className = '',
}: {
  title: string;
  subtitle?: string;
  right?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <article className={`overflow-hidden rounded-2xl border border-border bg-card shadow-sm ${className}`}>
      <div className="flex min-h-13 items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div>
          <div className="text-[13px] font-semibold text-foreground">{title}</div>
          {subtitle ? <div className="mt-1 text-[11px] text-muted-foreground">{subtitle}</div> : null}
        </div>
        {right}
      </div>
      <div className="p-4">{children}</div>
    </article>
  );
}

function PlaceholderCard({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <div className="rounded-xl border border-dashed border-[#d8dee7] bg-[#fbfcfe] px-4 py-6 text-sm text-[#667085]">
      <div className="font-medium text-[#344054]">{title}</div>
      <p className="mt-2 leading-6">{description}</p>
    </div>
  );
}

function KpiCard({
  label,
  badge,
  value,
  subtext,
  tone,
}: {
  label: string;
  badge: string;
  value: string;
  subtext: ReactNode;
  tone: Tone;
}) {
  const palette = toneClasses(tone);

  return (
    <article className={`relative min-h-30 overflow-hidden rounded-2xl border p-4 shadow-sm ${palette.card}`}>
      <div className={`pointer-events-none absolute -right-7 -bottom-8 h-22 w-22 rounded-full ${palette.glow}`} />
      <div className="relative flex items-center justify-between gap-2">
        <div className="text-xs font-semibold text-muted-foreground">{label}</div>
        <span className={`rounded-md px-2 py-0.5 text-[10px] font-bold ${palette.badge}`}>{badge}</span>
      </div>
      <div className={`relative mt-4 text-3xl font-semibold tracking-[-0.04em] ${palette.value}`}>{value}</div>
      <div className="relative mt-3 text-[11px] text-[#98a2b3]">{subtext}</div>
    </article>
  );
}

function PercentileBlock({
  title,
  status,
  tone,
  metrics,
}: {
  title: string;
  status: string;
  tone: 'orange' | 'cyan';
  metrics: FlowMetricsPercentiles;
}) {
  const toneClass = tone === 'orange' ? 'text-[#f08b32]' : 'text-[#0ca8bd]';

  return (
    <div className="rounded-xl border border-[#edf0f4] bg-[#fbfcfe] p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <div className="text-[11px] font-semibold text-[#475467]">{title}</div>
        <div className="text-[10px] text-[#98a2b3]">{status}</div>
      </div>
      <div className="grid grid-cols-3 gap-3">
        {[
          { label: 'P50', value: metrics.p50 },
          { label: 'P90', value: metrics.p90 },
          { label: 'P99', value: metrics.p99 },
        ].map((item, index) => (
          <div key={item.label} className={index < 2 ? 'border-r border-[#e9edf2]' : ''}>
            <small className="block text-[10px] text-[#98a2b3]">{item.label}</small>
            <strong className={`mt-1 block text-lg tracking-[-0.02em] ${toneClass}`}>{formatPercentile(item.value)}</strong>
          </div>
        ))}
      </div>
      <div className="mt-3 text-[10px] text-[#98a2b3]">样本数：{formatNumber(metrics.sample_count)}</div>
    </div>
  );
}

function MetricList({ rows }: { rows: MetricRow[] }) {
  return (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
      {rows.map((row) => (
        <Fragment key={row.label}>
          <span className="text-[#98a2b3]">{row.label}</span>
          <span className="text-right text-[#475467]">{row.value}</span>
        </Fragment>
      ))}
    </div>
  );
}

export default function FlowTowerContent() {
  const [selectedRange, setSelectedRange] = useState<RangeKey>('15m');
  const [selectedModelId, setSelectedModelId] = useState('all');
  const [modelSearch, setModelSearch] = useState('');
  const [activeTab, setActiveTab] = useState<FlowTabKey>('flow');
  const [queryNowMs, setQueryNowMs] = useState(() => Date.now());
  const lastSuccessfulMetricsRef = useRef<ReturnType<typeof useFlowMetrics>['data']>(undefined);

  const rangeBounds = useMemo(() => formatRangeBounds(selectedRange, queryNowMs), [selectedRange, queryNowMs]);

  const modelsQuery = useModels();
  const channelsQuery = useChannels();
  const routingHealthQuery = useRoutingHealth();
  const channelById = useMemo(
    () => new Map((channelsQuery.data ?? []).map((channel) => [channel.id, channel])),
    [channelsQuery.data],
  );
  const healthByModelId = useMemo(
    () => new Map((routingHealthQuery.data?.models ?? []).map((model) => [model.id, model])),
    [routingHealthQuery.data?.models],
  );
  const catalogModels = useMemo(
    () => (modelsQuery.data ?? [])
      .map((model) => deriveCatalogModel(model, channelById, !channelsQuery.isLoading && !channelsQuery.isError, healthByModelId.get(model.id)))
      .sort((left, right) => left.config.name.localeCompare(right.config.name) || left.config.id.localeCompare(right.config.id)),
    [channelById, channelsQuery.isLoading, healthByModelId, modelsQuery.data],
  );
  const selectedCatalogModel = selectedModelId === 'all'
    ? undefined
    : catalogModels.find((model) => model.config.id === selectedModelId);
  const modelParam = selectedCatalogModel?.config.name;
  const flowMetrics = useFlowMetrics(
    {
      start: rangeBounds.start,
      end: rangeBounds.end,
      model: modelParam,
    },
    {
      refetchInterval: false,
    },
  );

  useEffect(() => {
    const timer = window.setInterval(() => {
      setQueryNowMs(Date.now());
    }, 30_000);

    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (flowMetrics.data && !flowMetrics.isPlaceholderData) {
      lastSuccessfulMetricsRef.current = flowMetrics.data;
    }
  }, [flowMetrics.data, flowMetrics.isPlaceholderData]);

  useEffect(() => {
    if (selectedModelId !== 'all' && !catalogModels.some((model) => model.config.id === selectedModelId)) {
      setSelectedModelId('all');
    }
  }, [catalogModels, selectedModelId]);

  const visibleModels = useMemo(() => {
    const keyword = modelSearch.trim().toLowerCase();
    if (!keyword) return catalogModels;
    return catalogModels.filter((model) => [
      model.config.name,
      model.config.id,
      model.config.model_pattern,
      ...model.config.channels.map((channel) => channel.channel_id),
      ...model.config.channels.flatMap((channel) => channel.upstream_model ? [channel.upstream_model] : []),
    ].some((value) => value.toLowerCase().includes(keyword)));
  }, [catalogModels, modelSearch]);

  const effectiveFlowMetricsData = flowMetrics.data ?? lastSuccessfulMetricsRef.current;

  const historical = effectiveFlowMetricsData?.historical;
  const realtime = effectiveFlowMetricsData?.realtime;
  const displayedRange = effectiveFlowMetricsData?.range;
  const displayedRangeModel = displayedRange?.model ?? null;
  const displayedRangeLabel = inferRangeLabel(displayedRange?.start, displayedRange?.end);
  const displayedModelLabel = displayedRangeModel ?? '全部模型';
  const isShowingPreviousMetrics = flowMetrics.isPlaceholderData;
  const totalCompleted = historical?.total_completed ?? 0;
  const successRate = totalCompleted > 0
    ? (historical?.success_completed ?? 0) / totalCompleted * 100
    : null;
  const failureRate = totalCompleted > 0
    ? (historical?.failed_completed ?? 0) / totalCompleted * 100
    : null;

  const queueUnavailable = realtime?.queue.count == null;
  const queueValue = queueUnavailable ? '—' : formatNumber(realtime?.queue.count);
  const queueSubtext = queueUnavailable
    ? `当前不可用 · ${realtime?.queue.reason ?? '待后端支持'}`
    : `状态：${realtime?.queue.status}`;

  const shareRows: FlowMetricsModelShare[] = historical?.model_share ?? [];
  const ipRows = useMemo(
    () => formatIpRows(historical?.client_ips ?? []),
    [historical?.client_ips],
  );
  const trendData = useMemo(() => {
    const trend = historical?.trend;
    if (!trend) return [];
    return trend.buckets.map((bucket, index) => ({
      bucket,
      label: formatTrendBucket(bucket, trend.bucket_unit),
      success_completed: trend.success_completed[index] ?? 0,
      failed_completed: trend.failed_completed[index] ?? 0,
    }));
  }, [historical?.trend]);

  const historicalInspectorRows: MetricRow[] = [
    { label: '统计区间', value: displayedRangeLabel },
    { label: '筛选模型', value: displayedModelLabel },
    { label: '完成请求', value: formatNumber(totalCompleted) },
    { label: '成功完成', value: formatNumber(historical?.success_completed) },
    { label: '失败完成', value: formatNumber(historical?.failed_completed) },
    { label: '成功率', value: formatPercent(successRate) },
    { label: 'P99 延迟', value: historical?.latency_ms.p99 != null ? `${formatPercentile(historical.latency_ms.p99)} ms` : '—' },
    { label: 'TTFT P99', value: historical?.ttft_ms.p99 != null ? `${formatPercentile(historical.ttft_ms.p99)} ms` : '—' },
  ];

  const lastUpdatedLabel = realtime?.as_of
    ? new Date(realtime.as_of).toLocaleTimeString('zh-CN', { hour12: false })
    : '—';

  return (
    <div className="space-y-4 animate-fade-in">
      <section className="overflow-hidden rounded-2xl border border-border bg-[linear-gradient(180deg,rgba(255,255,255,0.98)_0%,rgba(248,250,252,0.96)_100%)] shadow-sm">
        <div className="flex flex-col gap-4 border-b border-border px-5 py-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
            <p className="mt-1 text-sm text-muted-foreground">实时请求态势、模型流量、端点健康与异常定位</p>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-[#475467]">
              自动刷新 · 30s
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-[#475467]">
              最近更新 · {lastUpdatedLabel}
            </div>
          </div>
        </div>

        <div className="space-y-4 px-5 py-5">
          <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
            <div className="flex flex-wrap items-center gap-2">
              <div className="inline-flex rounded-xl bg-[#eaf0f7] p-1">
                {RANGE_OPTIONS.map((option) => (
                  <button
                    key={option.key}
                    type="button"
                    onClick={() => {
                      setSelectedRange(option.key);
                      setQueryNowMs(Date.now());
                    }}
                    className={`rounded-lg px-3 py-1.5 text-xs transition ${
                      selectedRange === option.key
                        ? 'bg-white text-[#1f2937] shadow-sm'
                        : 'text-[#697586] hover:text-[#1f2937]'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
              <select
                aria-label="模型筛选"
                className="h-9 rounded-lg border border-border bg-background px-3 text-xs text-[#475467]"
                value={selectedModelId}
                onChange={(event) => {
                  setSelectedModelId(event.target.value);
                  setQueryNowMs(Date.now());
                }}
              >
                <option value="all">全部模型</option>
                {catalogModels.map((model) => (
                  <option key={model.config.id} value={model.config.id}>
                    {model.config.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label="渠道筛选"
                className="h-9 rounded-lg border border-border bg-muted px-3 text-xs text-[#98a2b3]"
                value="pending"
                disabled
              >
                <option value="pending">渠道筛选（待接口支持）</option>
              </select>
              <button
                type="button"
                onClick={() => setQueryNowMs(Date.now())}
                className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-xs text-[#475467] transition hover:bg-muted"
              >
                {flowMetrics.isFetching ? '刷新中…' : '↻ 刷新'}
              </button>
              {isShowingPreviousMetrics ? (
                <span className="text-[10px] text-[#98a2b3]">正在更新数据…</span>
              ) : null}
            </div>
          </div>

          <section className="grid gap-3 md:grid-cols-2 2xl:grid-cols-6">
            <KpiCard
              label="当前在途请求数"
              badge="LIVE"
              value={formatNumber(realtime?.in_flight)}
              tone="blue"
              subtext="当前正在处理的请求数"
            />
            <KpiCard
              label="上游生成中"
              badge="LIVE"
              value={formatNumber(realtime?.upstream_generating)}
              tone="cyan"
              subtext="尚未收到首字节的上游请求数"
            />
            <KpiCard
              label="上游输出中"
              badge="LIVE"
              value={formatNumber(realtime?.upstream_outputting)}
              tone="green"
              subtext="已经开始返回内容的上游请求数"
            />
            <KpiCard
              label="排队请求数"
              badge="PENDING"
              value={queueValue}
              tone="yellow"
              subtext={queueSubtext}
            />
            <KpiCard
              label="成功完成"
              badge={displayedRangeLabel}
              value={formatNumber(historical?.success_completed)}
              tone="green"
              subtext={<>成功率 <span className="font-semibold text-[#16a36a]">{formatPercent(successRate)}</span></>}
            />
            <KpiCard
              label="失败完成"
              badge={displayedRangeLabel}
              value={formatNumber(historical?.failed_completed)}
              tone="red"
              subtext={<>错误率 <span className="font-semibold text-[#e24f4f]">{formatPercent(failureRate)}</span></>}
            />
          </section>

          <section className="grid gap-4 2xl:grid-cols-[minmax(0,1.2fr)_minmax(380px,0.8fr)]">
            <div className="grid gap-4">
              <Panel
                title="成功 / 失败完成量趋势"
                subtitle={`最近 ${displayedRangeLabel} · ${historical?.trend.bucket_unit === 'minute' ? '按分钟聚合' : '按小时聚合'}`}
              >
                {trendData.length > 0 ? (
                  <div className="h-[260px] w-full">
                    <ResponsiveContainer>
                      <AreaChart data={trendData} margin={{ top: 8, right: 8, left: -18, bottom: 0 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#edf1f6" />
                        <XAxis dataKey="label" tick={{ fill: '#98a2b3', fontSize: 10 }} minTickGap={20} />
                        <YAxis tick={{ fill: '#98a2b3', fontSize: 10 }} allowDecimals={false} />
                        <RechartsTooltip
                          contentStyle={{ borderRadius: 12, border: '1px solid #e6ebf2', fontSize: 12 }}
                          formatter={(value, name) => [formatNumber(Number(value)), name === 'success_completed' ? '成功完成' : '失败完成']}
                          labelFormatter={(label) => String(label ?? '')}
                        />
                        <Area type="monotone" dataKey="success_completed" stackId="1" stroke="#27ad74" fill="#54bd8b" fillOpacity={0.25} />
                        <Area type="monotone" dataKey="failed_completed" stackId="1" stroke="#e45d5d" fill="#e45d5d" fillOpacity={0.18} />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                ) : (
                  <div className="h-[260px] w-full rounded-xl border border-dashed border-[#d8dee7] bg-[#fbfcfe]" />
                )}
              </Panel>

              <Panel
                title="客户端 IP Top N"
                subtitle={`请求来源排行 · 最近 ${displayedRangeLabel}`}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-[#475467]">
                    按请求数
                  </div>
                }
              >
                {ipRows.length > 0 ? (
                  <div className="overflow-x-auto">
                    <table className="w-full min-w-[520px] border-collapse">
                      <thead>
                        <tr>
                          <th className="pb-2 text-left text-[10px] font-semibold text-[#98a2b3]">客户端 IP</th>
                          <th className="pb-2 text-left text-[10px] font-semibold text-[#98a2b3]">请求占比</th>
                          <th className="pb-2 text-right text-[10px] font-semibold text-[#98a2b3]">请求</th>
                        </tr>
                      </thead>
                      <tbody>
                        {ipRows.map((row) => (
                          <tr key={row.ip}>
                            <td className="border-t border-[#f2f4f7] py-2.5 font-mono text-[11px] text-[#475467]">{row.ip}</td>
                            <td className="border-t border-[#f2f4f7] py-2.5">
                              <div className="h-1.5 overflow-hidden rounded-full bg-[#eef2f7]">
                                <div
                                  className="h-full rounded-full bg-[linear-gradient(90deg,#9683ed,#5f7fe5)]"
                                  style={{ width: `${row.ratio}%` }}
                                />
                              </div>
                            </td>
                            <td className="border-t border-[#f2f4f7] py-2.5 text-right text-[11px] text-[#475467]">{formatNumber(row.requests)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                ) : (
                  <PlaceholderCard title="暂无客户端 IP 数据" description="当前区间内没有可用的 client_ip 统计，或观测数据尚未写入。" />
                )}
              </Panel>
            </div>

            <div className="grid gap-4">
              <Panel
                title="模型请求占比"
                subtitle={`成功 + 失败请求总量 · 最近 ${displayedRangeLabel}`}
                right={<div className="text-[11px] text-[#98a2b3]">{formatNumber(totalCompleted)} requests</div>}
              >
                {shareRows.length > 0 ? (
                  <div className="flex flex-col gap-3">
                    {shareRows.map((row) => (
                      <div key={row.model} className="grid grid-cols-[minmax(120px,145px)_minmax(0,1fr)_52px] items-center gap-3">
                        <div className="truncate text-[11px] text-[#475467]">{row.model}</div>
                        <div className="h-2 overflow-hidden rounded-full bg-[#eef2f7]">
                          <div
                            className="h-full rounded-full bg-[linear-gradient(90deg,#7eb0ff,#3678e8)]"
                            style={{ width: `${Math.max(6, row.share)}%` }}
                          />
                        </div>
                        <div className="text-right text-[11px] text-[#667085]">{row.share.toFixed(1)}%</div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <PlaceholderCard title="暂无模型占比数据" description="当前区间内没有模型完成请求统计，暂时无法生成模型请求占比。" />
                )}
              </Panel>

              <Panel
                title="延迟 / TTFT 分位数"
                subtitle={`全模型汇总 · 最近 ${displayedRangeLabel}`}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-[#475467]">
                    ms
                  </div>
                }
              >
                <div className="grid gap-3">
                  <PercentileBlock
                    title="请求延迟"
                    status="来自 ClickHouse usage_events"
                    tone="orange"
                    metrics={historical?.latency_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                  />
                  <PercentileBlock
                    title="TTFT"
                    status="仅统计已有首字节观测样本"
                    tone="cyan"
                    metrics={historical?.ttft_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                  />
                </div>
              </Panel>
            </div>
          </section>

          <section className="grid gap-4 xl:grid-cols-[240px_minmax(0,1fr)] 2xl:grid-cols-[240px_minmax(0,1fr)_310px]">
            <Panel
              title="模型目录"
              subtitle={`${formatNumber(catalogModels.length)} 个已配置模型 · 结合 24h routing health`}
              className="min-h-[580px]"
              right={routingHealthQuery.isFetching ? <span className="text-[10px] text-[#98a2b3]">更新中…</span> : null}
            >
              <div className="-mt-1">
                <div className="mb-3">
                  <input
                    aria-label="搜索模型"
                    value={modelSearch}
                    onChange={(event) => setModelSearch(event.target.value)}
                    placeholder="搜索模型、ID 或渠道"
                    className="h-9 w-full rounded-lg border border-[#e1e7ef] bg-[#fbfcfe] px-3 text-[11px] text-[#475467] outline-none placeholder:text-[#98a2b3]"
                  />
                </div>
                {routingHealthQuery.isError ? (
                  <button
                    type="button"
                    onClick={() => void routingHealthQuery.refetch()}
                    className="mb-3 w-full rounded-lg border border-dashed border-[#d8dee7] px-3 py-2 text-[10px] text-[#667085] hover:bg-muted"
                  >
                    路由健康数据加载失败，点击重试
                  </button>
                ) : null}
                <div className="space-y-1">
                  <button
                    type="button"
                    onClick={() => {
                      setSelectedModelId('all');
                      setQueryNowMs(Date.now());
                    }}
                    className={`w-full rounded-xl border px-3 py-3 text-left transition ${
                      selectedModelId === 'all'
                        ? 'border-[#d8e6ff] bg-[#eff5ff]'
                        : 'border-transparent hover:bg-[#f7f9fc]'
                    }`}
                  >
                    <div className="text-[11px] font-semibold text-[#344054]">全部模型</div>
                    <div className="mt-2 text-[10px] text-[#98a2b3]">查看当前范围聚合与全局实时快照</div>
                  </button>
                  {visibleModels.length > 0 ? (
                    visibleModels.map((model) => {
                      const health = modelHealthPresentation(model.status);
                      return (
                        <button
                          key={model.config.id}
                          type="button"
                          onClick={() => {
                            setSelectedModelId(model.config.id);
                            setQueryNowMs(Date.now());
                          }}
                          className={`w-full rounded-xl border px-3 py-3 text-left transition ${
                            selectedModelId === model.config.id
                              ? 'border-[#d8e6ff] bg-[#eff5ff]'
                              : 'border-transparent hover:bg-[#f7f9fc]'
                          }`}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <div className="truncate text-[11px] font-semibold text-[#344054]">{model.config.name}</div>
                            <span className="inline-flex items-center gap-1.5 text-[10px] text-[#667085]">
                              <i aria-hidden="true" className={`h-2 w-2 rounded-sm ${health.dot}`} />
                              {health.label}
                            </span>
                          </div>
                          <div className="mt-2 flex flex-wrap gap-2 text-[10px] text-[#98a2b3]">
                            <span>{model.config.channels.length} configured channels</span>
                            {model.routedRequests24h == null ? (
                              <span>暂无路由健康数据</span>
                            ) : (
                              <>
                                <span>{formatNumber(model.routedRequests24h)} req / 24h</span>
                                <span>{formatPercent(model.successRate24h == null ? null : model.successRate24h * 100)}</span>
                              </>
                            )}
                          </div>
                        </button>
                      );
                    })
                  ) : modelsQuery.isLoading ? (
                    <div className="rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3]">
                      正在加载模型列表…
                    </div>
                  ) : modelsQuery.isError ? (
                    <button
                      type="button"
                      onClick={() => void modelsQuery.refetch()}
                      className="w-full rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3] hover:bg-muted"
                    >
                      模型列表加载失败，点击重试
                    </button>
                  ) : (
                    <div className="rounded-xl border border-dashed border-[#d8dee7] px-3 py-6 text-center text-[11px] text-[#98a2b3]">
                      没有匹配的模型
                    </div>
                  )}
                </div>
              </div>
            </Panel>

            <article className="overflow-hidden rounded-2xl border border-border bg-card shadow-sm">
              <div className="flex gap-4 border-b border-border px-4 pt-3" role="tablist" aria-label="流控台详情标签页">
                {FLOW_TABS.map((tab) => (
                  <button
                    key={tab.key}
                    id={`flowtower-tab-${tab.key}`}
                    type="button"
                    role="tab"
                    aria-selected={activeTab === tab.key}
                    aria-controls={`flowtower-panel-${tab.key}`}
                    onClick={() => setActiveTab(tab.key)}
                    className={`border-b-2 pb-3 text-[11px] transition ${
                      activeTab === tab.key
                        ? 'border-[#2f6edb] text-[#2f6edb] font-semibold'
                        : 'border-transparent text-[#667085] hover:text-[#2f6edb]'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {activeTab === 'flow' ? (
                <div id="flowtower-panel-flow" role="tabpanel" aria-labelledby="flowtower-tab-flow" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="请求流拓扑待第二阶段接入"
                    description="路由路径与端点分布正在完善中。"
                  />
                </div>
              ) : null}

              {activeTab === 'endpoint' ? (
                <div id="flowtower-panel-endpoint" role="tabpanel" aria-labelledby="flowtower-tab-endpoint" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="端点状态待第二阶段接入"
                    description="端点状态详情正在完善中。"
                  />
                </div>
              ) : null}

              {activeTab === 'compare' ? (
                <div id="flowtower-panel-compare" role="tabpanel" aria-labelledby="flowtower-tab-compare" className="space-y-5 p-4">
                  <PlaceholderCard
                    title="模型对比待第二阶段接入"
                    description="模型对比详情正在完善中。"
                  />
                </div>
              ) : null}
            </article>

            <Panel
              title="模型检查器"
              subtitle={selectedCatalogModel ? '模型配置、24h routing health 与选定区间历史指标' : '当前范围聚合指标与全局实时快照'}
              className="min-h-[580px]"
            >
              {selectedCatalogModel ? (() => {
                const health = modelHealthPresentation(selectedCatalogModel.status);
                const categories = selectedCatalogModel.config.category?.split(',').map((item) => item.trim()).filter(Boolean) ?? [];
                const routingRows: MetricRow[] = [
                  { label: '路由状态', value: health.label },
                  { label: '可用端点', value: `${selectedCatalogModel.availableEndpoints} / ${selectedCatalogModel.enabledEndpoints}` },
                  { label: '健康观测端点', value: `${selectedCatalogModel.observedEndpoints} / ${selectedCatalogModel.enabledEndpoints}` },
                  { label: '24h 路由请求', value: selectedCatalogModel.routedRequests24h == null ? '—' : formatNumber(selectedCatalogModel.routedRequests24h) },
                  { label: '24h 成功率', value: formatPercent(selectedCatalogModel.successRate24h == null ? null : selectedCatalogModel.successRate24h * 100) },
                  { label: '24h 平均延迟', value: selectedCatalogModel.averageLatency24h == null ? '—' : `${formatNumber(Math.round(selectedCatalogModel.averageLatency24h))} ms` },
                  { label: '最高 channel P95', value: selectedCatalogModel.highestChannelP95 == null ? '—' : `${formatNumber(Math.round(selectedCatalogModel.highestChannelP95))} ms` },
                  { label: '熔断通道', value: String(selectedCatalogModel.brokenCircuitChannels) },
                ];
                return (
                  <div className="space-y-4">
                    <div className="rounded-xl border border-[#dfe7f3] bg-[#f8fbff] p-4">
                      <div className="flex items-center justify-between gap-2">
                        <div>
                          <strong className="text-sm text-[#182230]">{selectedCatalogModel.config.name}</strong>
                          <div className="mt-1 font-mono text-[10px] text-[#98a2b3]">{selectedCatalogModel.config.id}</div>
                        </div>
                        <span className={`rounded-md px-2 py-1 text-[10px] font-semibold ${health.badge}`}>{health.label}</span>
                      </div>
                      <div className="mt-3 flex flex-wrap gap-1.5">
                        <span className="rounded bg-white px-2 py-1 text-[10px] text-[#667085]">{selectedCatalogModel.config.published ? '已发布' : '未发布'}</span>
                        {categories.map((category) => <span key={category} className="rounded bg-white px-2 py-1 text-[10px] text-[#667085]">{category}</span>)}
                      </div>
                      <div className="mt-4 grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
                        <span className="text-[#98a2b3]">模型 Pattern</span><span className="max-w-[180px] truncate text-right text-[#475467]">{selectedCatalogModel.config.model_pattern}</span>
                        <span className="text-[#98a2b3]">上下文长度</span><span className="text-right text-[#475467]">{formatContextLength(selectedCatalogModel.config.context_length)}</span>
                        <span className="text-[#98a2b3]">配置通道</span><span className="text-right text-[#475467]">{selectedCatalogModel.config.channels.length}</span>
                      </div>
                    </div>

                    <div className="rounded-xl border border-[#edf0f4] bg-white p-4">
                      <div className="mb-3 flex items-center justify-between gap-2">
                        <div className="text-[11px] font-semibold text-[#475467]">Routing Health · 最近 24h</div>
                        {routingHealthQuery.isFetching ? <span className="text-[10px] text-[#98a2b3]">更新中…</span> : null}
                      </div>
                      {routingHealthQuery.isError ? (
                        <button type="button" onClick={() => void routingHealthQuery.refetch()} className="text-[10.5px] text-[#667085] underline">
                          路由健康数据加载失败，点击重试
                        </button>
                      ) : selectedCatalogModel.health ? (
                        <MetricList rows={routingRows} />
                      ) : (
                        <p className="text-[10.5px] leading-5 text-[#98a2b3]">该已配置模型当前没有可用的 routing health 记录；这不等于故障，可能是无流量或没有启用端点。</p>
                      )}
                      {selectedCatalogModel.config.channels.length > 0 ? (
                        <div className="mt-3 border-t border-[#edf0f4] pt-3">
                          <div className="mb-2 text-[10px] text-[#98a2b3]">配置通道绑定</div>
                          <div className="space-y-1.5">
                            {selectedCatalogModel.config.channels.map((channel) => (
                              <div key={`${channel.channel_id}-${channel.priority}`} className="flex justify-between gap-3 text-[10px] text-[#667085]">
                                <span className="truncate">{channel.channel_id}{channel.upstream_model ? ` → ${channel.upstream_model}` : ''}</span>
                                <span className="shrink-0">priority {channel.priority}</span>
                              </div>
                            ))}
                          </div>
                        </div>
                      ) : null}
                    </div>

                    <div className="rounded-xl border border-[#edf0f4] bg-white p-4">
                      <div className="mb-3 text-[11px] font-semibold text-[#475467]">Flow Metrics · 当前选择区间</div>
                      <MetricList rows={historicalInspectorRows} />
                    </div>
                  </div>
                );
              })() : (
                <div className="space-y-4">
                  <div className="rounded-xl border border-[#dfe7f3] bg-[#f8fbff] p-4">
                    <div className="flex items-center justify-between gap-2">
                      <strong className="text-sm text-[#182230]">全部模型</strong>
                      <span className="rounded-md bg-[#edf4ff] px-2 py-1 text-[10px] font-semibold text-[#3276e8]">聚合视图</span>
                    </div>
                    <div className="mt-4"><MetricList rows={historicalInspectorRows} /></div>
                  </div>
                  <p className="rounded-xl border border-dashed border-[#d8dee7] bg-[#fbfcfe] px-4 py-3 text-[10.5px] leading-5 text-[#667085]">选择左侧具体模型后，可查看模型配置、最近 24 小时运行状态和端点可用性。</p>
                </div>
              )}

              <div className="mt-4 border-t border-border pt-4">
                <PlaceholderCard
                  title="异常事件待后端聚合接口"
                  description="当前仓库里还没有统一的 incidents feed 用于支撑“最近 30 分钟异常事件”。等后端补充事件聚合接口后，再把这个区域切成 real。"
                />
              </div>
            </Panel>
          </section>
        </div>
      </section>
    </div>
  );
}
