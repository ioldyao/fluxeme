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
import { usePublicModels } from '@fluxeme/shared/src/api/models';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { useProbeResults } from '@fluxeme/shared/src/api/probe';
import { useRoutingHealth } from '@fluxeme/shared/src/api/routing';
import type { RoutingHealthModel } from '@fluxeme/shared/src/api/routing';
import RoutingFlow from './RoutingFlow';
import type { Channel, FlowMetricsClientIp, FlowMetricsModelShare, FlowMetricsPercentiles, Model, ProbeResult } from '@fluxeme/shared/src/types';

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

type EndpointStatusRow = {
  channelId: string;
  channelName: string;
  channelPriority: number;
  provider?: string;
  endpointId: number | null;
  endpointUrl: string;
  endpointEnabled: boolean;
  endpointWeight: number;
  endpointTimeoutSecs?: number | null;
  routingObserved: boolean;
  routingAvailable: boolean;
  probe: ProbeResult | null;
  channelRequests24h: number;
  channelSuccessRate24h?: number;
  channelP95Latency24h?: number;
  circuitEnabled: boolean;
  circuitOk: boolean;
};

type CompareRow = {
  id: string;
  name: string;
  status: string;
  statusBadge: string;
  selectedRangeRequests: number | null;
  selectedRangeShare: number | null;
  routedRequests24h?: number;
  routingSuccessRate24h: number | null;
  averageLatency24h?: number;
  highestChannelP95?: number;
  availableEndpoints: number;
  enabledEndpoints: number;
  brokenCircuitChannels: number;
  configuredChannels: number;
  selected: boolean;
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

function formatDateTime(value?: string | null) {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString('zh-CN', { hour12: false });
}

function endpointRoutingLabel(row: EndpointStatusRow) {
  if (!row.routingObserved) return '未观测';
  return row.routingAvailable ? '可路由' : '不可路由';
}

function endpointProbeLabel(probe: ProbeResult | null) {
  if (!probe) return '未探测';
  if (probe.success) return `成功 · ${formatNumber(probe.latency_ms)}ms`;
  return '失败';
}

function modelHealthPresentation(status: ModelHealthStatus) {
  switch (status) {
    case 'healthy':
      return { label: '可路由', dot: 'bg-chart-2', badge: 'bg-chart-2/15 text-chart-2' };
    case 'degraded':
      return { label: '部分降级', dot: 'bg-sidebar-primary', badge: 'bg-sidebar-primary/15 text-sidebar-primary' };
    case 'unavailable':
      return { label: '不可用', dot: 'bg-destructive', badge: 'bg-destructive/15 text-destructive' };
    case 'unknown':
      return { label: '健康未知', dot: 'bg-muted-foreground', badge: 'bg-muted text-muted-foreground' };
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
        card: 'border-accent bg-[linear-gradient(180deg,var(--card)_0%,var(--accent)_100%)]',
        badge: 'bg-accent text-accent-foreground',
        value: 'text-accent-foreground',
        glow: 'bg-accent',
      };
    case 'cyan':
      return {
        card: 'border-accent bg-[linear-gradient(180deg,var(--card)_0%,var(--accent)_100%)]',
        badge: 'bg-accent text-chart-1',
        value: 'text-chart-1',
        glow: 'bg-accent',
      };
    case 'green':
      return {
        card: 'border-chart-2/15 bg-[linear-gradient(180deg,var(--card)_0%,var(--accent)_100%)]',
        badge: 'bg-chart-2/15 text-chart-2',
        value: 'text-chart-2',
        glow: 'bg-chart-2/15',
      };
    case 'yellow':
      return {
        card: 'border-sidebar-primary/15 bg-[linear-gradient(180deg,var(--card)_0%,color-mix(in oklab,var(--card)_78%,var(--sidebar-primary))_100%)]',
        badge: 'bg-sidebar-primary/15 text-sidebar-primary',
        value: 'text-sidebar-primary',
        glow: 'bg-sidebar-primary/15',
      };
    case 'red':
      return {
        card: 'border-destructive/15 bg-[linear-gradient(180deg,var(--card)_0%,color-mix(in oklab,var(--card)_78%,var(--destructive))_100%)]',
        badge: 'bg-destructive/15 text-destructive',
        value: 'text-destructive',
        glow: 'bg-destructive/15',
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
    <div className="rounded-xl border border-dashed border-border bg-muted px-4 py-6 text-sm text-muted-foreground">
      <div className="font-medium text-muted-foreground">{title}</div>
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
        <div className="text-xs font-semibold text-foreground">{label}</div>
        <span className={`rounded-md px-2 py-0.5 text-[10px] font-bold ${palette.badge}`}>{badge}</span>
      </div>
      <div className={`relative mt-4 text-3xl font-semibold tracking-[-0.04em] ${palette.value}`}>{value}</div>
      <div className="relative mt-3 text-[11px] text-muted-foreground">{subtext}</div>
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
  const toneClass = tone === 'orange' ? 'text-sidebar-primary' : 'text-chart-1';

  return (
    <div className="rounded-xl border border-secondary bg-secondary p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <div className="text-[11px] font-semibold text-foreground">{title}</div>
        <div className="text-[10px] text-muted-foreground">{status}</div>
      </div>
      <div className="grid grid-cols-3 gap-3">
        {[
          { label: 'P50', value: metrics.p50 },
          { label: 'P90', value: metrics.p90 },
          { label: 'P99', value: metrics.p99 },
        ].map((item, index) => (
          <div key={item.label} className={index < 2 ? 'border-r border-border' : ''}>
            <small className="block text-[10px] text-muted-foreground">{item.label}</small>
            <strong className={`mt-1 block text-lg tracking-[-0.02em] ${toneClass}`}>{formatPercentile(item.value)}</strong>
          </div>
        ))}
      </div>
      <div className="mt-3 text-[10px] text-muted-foreground">样本数：{formatNumber(metrics.sample_count)}</div>
    </div>
  );
}

function MetricList({ rows }: { rows: MetricRow[] }) {
  return (
    <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
      {rows.map((row) => (
        <Fragment key={row.label}>
          <span className="text-muted-foreground">{row.label}</span>
          <span className="text-right font-medium text-foreground">{row.value}</span>
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

  const modelsQuery = usePublicModels();
  const channelsQuery = useChannels();
  const routingHealthQuery = useRoutingHealth();
  const probeResultsQuery = useProbeResults({
    enabled: selectedModelId !== 'all',
    modelId: selectedModelId !== 'all' ? selectedModelId : undefined,
  });
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
  const compareMetricsQuery = useFlowMetrics(
    {
      start: rangeBounds.start,
      end: rangeBounds.end,
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

  const endpointRows = useMemo(() => {
    if (!selectedCatalogModel) return [] as EndpointStatusRow[];

    const healthChannels = new Map((selectedCatalogModel.health?.channels ?? []).map((channel) => [channel.channel_id, channel]));
    const probeRows = (probeResultsQuery.data ?? []).filter((row) => row.model_id === selectedCatalogModel.config.id);

    return selectedCatalogModel.config.channels.flatMap((binding) => {
      const channel = channelById.get(binding.channel_id);
      if (!channel?.enabled) return [];
      const channelHealth = healthChannels.get(binding.channel_id);
      return channel.endpoints
        .filter((endpoint) => endpoint.enabled !== false)
        .map((endpoint) => {
          const endpointHealth = channelHealth?.endpoints.find((item) => item.endpoint_id === endpoint.id);
          const probe = probeRows.find((row) => row.channel_id === binding.channel_id && row.endpoint_url === endpoint.url) ?? null;
          return {
            channelId: binding.channel_id,
            channelName: channel.name || binding.channel_id,
            channelPriority: binding.priority,
            provider: channel.provider,
            endpointId: endpoint.id ?? null,
            endpointUrl: endpoint.url,
            endpointEnabled: endpoint.enabled !== false,
            endpointWeight: endpoint.weight,
            endpointTimeoutSecs: endpoint.timeout_secs,
            routingObserved: Boolean(endpointHealth),
            routingAvailable: endpointHealth?.available ?? false,
            probe,
            channelRequests24h: channelHealth?.requests ?? 0,
            channelSuccessRate24h: channelHealth?.requests ? channelHealth.success_rate * 100 : undefined,
            channelP95Latency24h: channelHealth?.requests ? channelHealth.p95_latency_ms : undefined,
            circuitEnabled: channelHealth?.circuit_enabled ?? false,
            circuitOk: channelHealth?.circuit_ok ?? false,
          } satisfies EndpointStatusRow;
        });
    });
  }, [channelById, probeResultsQuery.data, selectedCatalogModel]);

  const endpointSummaryRows: MetricRow[] = useMemo(() => {
    const enabled = endpointRows.length;
    const observed = endpointRows.filter((row) => row.routingObserved).length;
    const available = endpointRows.filter((row) => row.routingAvailable).length;
    const probeSuccess = endpointRows.filter((row) => row.probe?.success).length;
    const probeFail = endpointRows.filter((row) => row.probe && !row.probe.success).length;
    const probeUnknown = endpointRows.filter((row) => !row.probe).length;

    return [
      { label: '已启用端点', value: formatNumber(enabled) },
      { label: '已观测端点', value: formatNumber(observed) },
      { label: '当前可路由', value: formatNumber(available) },
      { label: '探测成功', value: formatNumber(probeSuccess) },
      { label: '探测失败', value: formatNumber(probeFail) },
      { label: '未探测', value: formatNumber(probeUnknown) },
    ];
  }, [endpointRows]);

  const compareModelShareRows = compareMetricsQuery.data?.historical.model_share ?? [];
  const compareModelShareMap = useMemo(
    () => new Map(compareModelShareRows.map((row) => [row.model, row])),
    [compareModelShareRows],
  );
  const unmatchedCompareModels = useMemo(
    () => compareModelShareRows.filter((row) => !catalogModels.some((model) => model.config.name === row.model)),
    [catalogModels, compareModelShareRows],
  );

  const compareRows = useMemo(() => {
    const rangeMetricsAvailable = !compareMetricsQuery.isError && compareMetricsQuery.data != null;
    return catalogModels.map((model) => {
      const health = modelHealthPresentation(model.status);
      const compareShare = compareModelShareMap.get(model.config.name);
      return {
        id: model.config.id,
        name: model.config.name,
        status: health.label,
        statusBadge: health.badge,
        selectedRangeRequests: rangeMetricsAvailable ? (compareShare?.requests ?? 0) : null,
        selectedRangeShare: rangeMetricsAvailable ? (compareShare?.share ?? 0) : null,
        routedRequests24h: model.routedRequests24h,
        routingSuccessRate24h: model.successRate24h == null ? null : model.successRate24h * 100,
        averageLatency24h: model.averageLatency24h,
        highestChannelP95: model.highestChannelP95,
        availableEndpoints: model.availableEndpoints,
        enabledEndpoints: model.enabledEndpoints,
        brokenCircuitChannels: model.brokenCircuitChannels,
        configuredChannels: model.config.channels.length,
        selected: selectedModelId === model.config.id,
      } satisfies CompareRow;
    });
  }, [catalogModels, compareMetricsQuery.data, compareMetricsQuery.isError, compareModelShareMap, selectedModelId]);

  const lastUpdatedLabel = realtime?.as_of
    ? new Date(realtime.as_of).toLocaleTimeString('zh-CN', { hour12: false })
    : '—';

  return (
    <div className="space-y-4 animate-fade-in">
      <section className="overflow-hidden rounded-2xl border border-border bg-[linear-gradient(180deg,var(--card)_0%,var(--muted)_100%)] shadow-sm">
        <div className="flex flex-col gap-4 border-b border-border px-5 py-4 lg:flex-row lg:items-center lg:justify-between">
          <div>
            <h2 className="text-lg font-semibold text-foreground">模型监控</h2>
            <p className="mt-1 text-sm text-muted-foreground">实时请求态势、模型流量、端点健康与异常定位</p>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-muted-foreground">
              自动刷新 · 30s
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-muted-foreground">
              最近更新 · {lastUpdatedLabel}
            </div>
          </div>
        </div>

        <div className="space-y-4 px-5 py-5">
          <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
            <div className="flex flex-wrap items-center gap-2">
              <div className="inline-flex rounded-xl bg-accent p-1">
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
                        ? 'bg-card text-foreground shadow-sm'
                        : 'text-muted-foreground hover:text-foreground'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
              <select
                aria-label="模型筛选"
                className="h-9 rounded-lg border border-border bg-background px-3 text-xs text-muted-foreground"
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
                className="h-9 rounded-lg border border-border bg-muted px-3 text-xs text-muted-foreground"
                value="pending"
                disabled
              >
                <option value="pending">渠道筛选（待接口支持）</option>
              </select>
              <button
                type="button"
                onClick={() => setQueryNowMs(Date.now())}
                className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-xs text-muted-foreground transition hover:bg-muted"
              >
                ↻ 刷新
              </button>
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
              subtext={<>成功率 <span className="font-semibold text-chart-2">{formatPercent(successRate)}</span></>}
            />
            <KpiCard
              label="失败完成"
              badge={displayedRangeLabel}
              value={formatNumber(historical?.failed_completed)}
              tone="red"
              subtext={<>错误率 <span className="font-semibold text-destructive">{formatPercent(failureRate)}</span></>}
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
                        <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
                        <XAxis dataKey="label" tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} minTickGap={20} />
                        <YAxis tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }} allowDecimals={false} />
                        <RechartsTooltip
                          contentStyle={{ borderRadius: 12, border: '1px solid var(--border)', fontSize: 12 }}
                          formatter={(value, name) => [formatNumber(Number(value)), name === 'success_completed' ? '成功完成' : '失败完成']}
                          labelFormatter={(label) => String(label ?? '')}
                        />
                        <Area type="monotone" dataKey="success_completed" stroke="var(--chart-2)" fill="var(--chart-2)" fillOpacity={0.25} />
                        <Area type="monotone" dataKey="failed_completed" stroke="var(--destructive)" fill="var(--destructive)" fillOpacity={0.18} />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                ) : (
                  <div className="h-[260px] w-full rounded-xl border border-dashed border-border bg-muted" />
                )}
              </Panel>

              <Panel
                title="客户端 IP Top N"
                subtitle={`请求来源排行 · 最近 ${displayedRangeLabel}`}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-muted-foreground">
                    按请求数
                  </div>
                }
              >
                {ipRows.length > 0 ? (
                  <div className="overflow-x-auto">
                    <table className="w-full min-w-[520px] border-collapse">
                      <thead>
                        <tr>
                          <th className="pb-2 text-left text-[10px] font-semibold text-muted-foreground">客户端 IP</th>
                          <th className="pb-2 text-left text-[10px] font-semibold text-muted-foreground">请求占比</th>
                          <th className="pb-2 text-right text-[10px] font-semibold text-muted-foreground">请求</th>
                        </tr>
                      </thead>
                      <tbody>
                        {ipRows.map((row) => (
                          <tr key={row.ip}>
                            <td className="border-t border-muted py-2.5 font-mono text-[11px] text-muted-foreground">{row.ip}</td>
                            <td className="border-t border-muted py-2.5">
                              <div className="h-1.5 overflow-hidden rounded-full bg-secondary">
                                <div
                                  className="h-full rounded-full bg-[linear-gradient(90deg,var(--chart-3),var(--accent-foreground))]"
                                  style={{ width: `${row.ratio}%` }}
                                />
                              </div>
                            </td>
                            <td className="border-t border-muted py-2.5 text-right text-[11px] text-muted-foreground">{formatNumber(row.requests)}</td>
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
                right={<div className="text-[11px] text-muted-foreground">{formatNumber(totalCompleted)} requests</div>}
              >
                {shareRows.length > 0 ? (
                  <div className="flex flex-col gap-3">
                    {shareRows.map((row) => (
                      <div key={row.model} className="grid grid-cols-[minmax(120px,145px)_minmax(0,1fr)_52px] items-center gap-3">
                        <div className="truncate text-[11px] font-medium text-foreground">{row.model}</div>
                        <div className="h-2 overflow-hidden rounded-full bg-secondary">
                          <div
                            className="h-full rounded-full bg-[linear-gradient(90deg,var(--chart-1),var(--accent-foreground))]"
                            style={{ width: `${Math.max(6, row.share)}%` }}
                          />
                        </div>
                        <div className="text-right text-[11px] font-semibold text-foreground">{row.share.toFixed(1)}%</div>
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
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-muted-foreground">
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
              right={routingHealthQuery.isFetching ? <span className="text-[10px] text-muted-foreground">更新中…</span> : null}
            >
              <div className="-mt-1">
                <div className="mb-3">
                  <input
                    aria-label="搜索模型"
                    value={modelSearch}
                    onChange={(event) => setModelSearch(event.target.value)}
                    placeholder="搜索模型、ID 或渠道"
                    className="h-9 w-full rounded-lg border border-border bg-muted px-3 text-[11px] text-muted-foreground outline-none placeholder:text-muted-foreground"
                  />
                </div>
                {routingHealthQuery.isError ? (
                  <button
                    type="button"
                    onClick={() => void routingHealthQuery.refetch()}
                    className="mb-3 w-full rounded-lg border border-dashed border-border px-3 py-2 text-[10px] text-muted-foreground hover:bg-muted"
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
                        ? 'border-accent bg-accent'
                        : 'border-transparent hover:bg-muted'
                    }`}
                  >
                    <div className="text-[11px] font-semibold text-muted-foreground">全部模型</div>
                    <div className="mt-2 text-[10px] text-muted-foreground">查看当前范围聚合与全局实时快照</div>
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
                              ? 'border-accent bg-accent'
                              : 'border-transparent hover:bg-muted'
                          }`}
                        >
                          <div className="flex items-center justify-between gap-2">
                            <div className="truncate text-[11px] font-semibold text-muted-foreground">{model.config.name}</div>
                            <span className="inline-flex items-center gap-1.5 text-[10px] text-muted-foreground">
                              <i aria-hidden="true" className={`h-2 w-2 rounded-sm ${health.dot}`} />
                              {health.label}
                            </span>
                          </div>
                          <div className="mt-2 flex flex-wrap gap-2 text-[10px] text-muted-foreground">
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
                    <div className="rounded-xl border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground">
                      正在加载模型列表…
                    </div>
                  ) : modelsQuery.isError ? (
                    <button
                      type="button"
                      onClick={() => void modelsQuery.refetch()}
                      className="w-full rounded-xl border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground hover:bg-muted"
                    >
                      模型列表加载失败，点击重试
                    </button>
                  ) : (
                    <div className="rounded-xl border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground">
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
                        ? 'border-accent-foreground text-accent-foreground font-semibold'
                        : 'border-transparent text-muted-foreground hover:text-accent-foreground'
                    }`}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>

              {activeTab === 'flow' ? (
                <div id="flowtower-panel-flow" role="tabpanel" aria-labelledby="flowtower-tab-flow" className="space-y-5 p-4">
                  {!selectedCatalogModel ? (
                    <PlaceholderCard
                      title="请选择模型查看请求流"
                      description="请求流按单个模型展示，先在左侧选择一个模型。"
                    />
                  ) : (
                    <RoutingFlow embedded modelName={selectedCatalogModel.config.name} />
                  )}
                </div>
              ) : null}

              {activeTab === 'endpoint' ? (
                <div id="flowtower-panel-endpoint" role="tabpanel" aria-labelledby="flowtower-tab-endpoint" className="space-y-5 p-4">
                  {!selectedCatalogModel ? (
                    <PlaceholderCard
                      title="请选择模型查看端点状态"
                      description="端点状态按单个模型聚合展示，先在左侧选择一个模型。"
                    />
                  ) : channelsQuery.isError ? (
                    <button
                      type="button"
                      onClick={() => void channelsQuery.refetch()}
                      className="w-full rounded-xl border border-dashed border-border px-4 py-6 text-center text-[11px] text-muted-foreground hover:bg-muted"
                    >
                      端点配置加载失败，点击重试
                    </button>
                  ) : endpointRows.length === 0 ? (
                    <PlaceholderCard
                      title="该模型绑定渠道下没有已启用端点"
                      description="请先检查该模型的渠道绑定和 endpoint 配置。"
                    />
                  ) : (
                    <>
                      <div className="grid gap-2 md:grid-cols-3 xl:grid-cols-6">
                        {endpointSummaryRows.map((row) => (
                          <div key={row.label} className="rounded-xl border border-secondary bg-card p-3">
                            <small className="text-[10px] text-muted-foreground">{row.label}</small>
                            <strong className="mt-1 block text-sm text-foreground">{row.value}</strong>
                          </div>
                        ))}
                      </div>

                      {routingHealthQuery.isError ? (
                        <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                          路由健康数据加载失败，当前只展示配置与探测结果。
                        </div>
                      ) : null}

                      {probeResultsQuery.isError ? (
                        <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                          探测结果加载失败，当前只展示配置与路由状态。
                        </div>
                      ) : null}

                      <div className="overflow-x-auto rounded-xl border border-secondary bg-card">
                        <table className="w-full min-w-[980px] border-collapse">
                          <thead>
                            <tr>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">渠道</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">端点</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">配置</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">路由状态</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">最近探测</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">最近探测时间</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 请求</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 成功率</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h P95</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">熔断</th>
                            </tr>
                          </thead>
                          <tbody>
                            {endpointRows.map((row) => (
                              <tr key={`${row.channelId}-${row.endpointId ?? row.endpointUrl}`}>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                  <div className="font-semibold">{row.channelName}</div>
                                  <div className="text-[10px] text-muted-foreground">{row.channelId}</div>
                                </td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                  <div className="max-w-[240px] truncate font-mono" title={row.endpointUrl}>{row.endpointUrl}</div>
                                  <div className="text-[10px] text-muted-foreground">ID: {row.endpointId ?? '—'}</div>
                                </td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                  <div>{row.endpointEnabled ? '已启用' : '已禁用'}</div>
                                  <div className="text-[10px] text-muted-foreground">weight {row.endpointWeight}{row.endpointTimeoutSecs != null ? ` · ${row.endpointTimeoutSecs}s` : ''}</div>
                                </td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{endpointRoutingLabel(row)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{endpointProbeLabel(row.probe)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{formatDateTime(row.probe?.probed_at)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatNumber(row.channelRequests24h)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatPercent(row.channelSuccessRate24h)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.channelP95Latency24h == null ? '—' : `${formatNumber(Math.round(row.channelP95Latency24h))} ms`}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{row.circuitEnabled ? (row.circuitOk ? '正常' : '熔断中') : '未启用'}</td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    </>
                  )}
                </div>
              ) : null}

              {activeTab === 'compare' ? (
                <div id="flowtower-panel-compare" role="tabpanel" aria-labelledby="flowtower-tab-compare" className="space-y-5 p-4">
                  <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                    左侧列为当前选择区间的完成请求；右侧列为最近 24 小时运行情况。
                  </div>
                  {compareMetricsQuery.isError ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      当前区间请求数据加载失败，已保留其他模型信息。
                    </div>
                  ) : null}
                  {routingHealthQuery.isError ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      24 小时运行情况加载失败，相关列暂不可用。
                    </div>
                  ) : null}
                  {unmatchedCompareModels.length > 0 ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      当前区间有 {formatNumber(unmatchedCompareModels.length)} 个未匹配到目录配置的模型请求记录。
                    </div>
                  ) : null}
                  {compareRows.length > 0 ? (
                    <div className="overflow-x-auto rounded-xl border border-secondary bg-card">
                      <table className="w-full min-w-[980px] border-collapse">
                        <thead>
                          <tr>
                            <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">模型</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">当前状态</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">区间请求数</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">区间占比</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 路由请求</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 路由成功率</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 平均延迟</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">24h 最高 P95</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">可用端点</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">熔断通道</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">配置通道</th>
                          </tr>
                        </thead>
                        <tbody>
                          {compareRows.map((row) => (
                            <tr key={row.id} className={row.selected ? 'bg-accent' : ''}>
                              <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                <div className="font-semibold">{row.name}</div>
                                <div className="text-[10px] text-muted-foreground">{row.selected ? '当前选中' : '—'}</div>
                              </td>
                              <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                <span className={`rounded-md px-2 py-1 text-[10px] font-semibold ${row.statusBadge}`}>{row.status}</span>
                              </td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.selectedRangeRequests == null ? '—' : formatNumber(row.selectedRangeRequests)}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.selectedRangeShare == null ? '—' : formatPercent(row.selectedRangeShare)}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.routedRequests24h == null ? '—' : formatNumber(row.routedRequests24h)}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatPercent(row.routingSuccessRate24h)}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.averageLatency24h == null ? '—' : `${formatNumber(Math.round(row.averageLatency24h))} ms`}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.highestChannelP95 == null ? '—' : `${formatNumber(Math.round(row.highestChannelP95))} ms`}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{`${row.availableEndpoints} / ${row.enabledEndpoints}`}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatNumber(row.brokenCircuitChannels)}</td>
                              <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatNumber(row.configuredChannels)}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  ) : (
                    <PlaceholderCard
                      title="暂无可对比模型"
                      description="当前没有可用于对比的模型配置或统计数据。"
                    />
                  )}
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
                    <div className="rounded-xl border border-border bg-accent p-4">
                      <div className="flex items-center justify-between gap-2">
                        <div>
                          <strong className="text-sm text-foreground">{selectedCatalogModel.config.name}</strong>
                          <div className="mt-1 font-mono text-[10px] text-muted-foreground">{selectedCatalogModel.config.id}</div>
                        </div>
                        <span className={`rounded-md px-2 py-1 text-[10px] font-semibold ${health.badge}`}>{health.label}</span>
                      </div>
                      <div className="mt-3 flex flex-wrap gap-1.5">
                        <span className="rounded bg-card px-2 py-1 text-[10px] text-muted-foreground">{selectedCatalogModel.config.published ? '已发布' : '未发布'}</span>
                        {categories.map((category) => <span key={category} className="rounded bg-card px-2 py-1 text-[10px] text-muted-foreground">{category}</span>)}
                      </div>
                      <div className="mt-4 grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
                        <span className="text-muted-foreground">模型 Pattern</span><span className="max-w-[180px] truncate text-right text-muted-foreground">{selectedCatalogModel.config.model_pattern}</span>
                        <span className="text-muted-foreground">上下文长度</span><span className="text-right text-muted-foreground">{formatContextLength(selectedCatalogModel.config.context_length)}</span>
                        <span className="text-muted-foreground">配置通道</span><span className="text-right text-muted-foreground">{selectedCatalogModel.config.channels.length}</span>
                      </div>
                    </div>

                    <div className="rounded-xl border border-secondary bg-card p-4">
                      <div className="mb-3 flex items-center justify-between gap-2">
                        <div className="text-[11px] font-semibold text-muted-foreground">Routing Health · 最近 24h</div>
                        {routingHealthQuery.isFetching ? <span className="text-[10px] text-muted-foreground">更新中…</span> : null}
                      </div>
                      {routingHealthQuery.isError ? (
                        <button type="button" onClick={() => void routingHealthQuery.refetch()} className="text-[10.5px] text-muted-foreground underline">
                          路由健康数据加载失败，点击重试
                        </button>
                      ) : selectedCatalogModel.health ? (
                        <MetricList rows={routingRows} />
                      ) : (
                        <p className="text-[10.5px] leading-5 text-muted-foreground">该已配置模型当前没有可用的 routing health 记录；这不等于故障，可能是无流量或没有启用端点。</p>
                      )}
                      {selectedCatalogModel.config.channels.length > 0 ? (
                        <div className="mt-3 border-t border-secondary pt-3">
                          <div className="mb-2 text-[10px] text-muted-foreground">配置通道绑定</div>
                          <div className="space-y-1.5">
                            {selectedCatalogModel.config.channels.map((channel) => (
                              <div key={`${channel.channel_id}-${channel.priority}`} className="flex justify-between gap-3 text-[10px] text-muted-foreground">
                                <span className="truncate">{channel.channel_id}{channel.upstream_model ? ` → ${channel.upstream_model}` : ''}</span>
                                <span className="shrink-0">priority {channel.priority}</span>
                              </div>
                            ))}
                          </div>
                        </div>
                      ) : null}
                    </div>

                    <div className="rounded-xl border border-secondary bg-card p-4">
                      <div className="mb-3 text-[11px] font-semibold text-muted-foreground">Flow Metrics · 当前选择区间</div>
                      <MetricList rows={historicalInspectorRows} />
                    </div>
                  </div>
                );
              })() : (
                <div className="space-y-4">
                  <div className="rounded-xl border border-border bg-accent p-4">
                    <div className="flex items-center justify-between gap-2">
                      <strong className="text-sm text-foreground">全部模型</strong>
                      <span className="rounded-md bg-accent px-2 py-1 text-[10px] font-semibold text-accent-foreground">聚合视图</span>
                    </div>
                    <div className="mt-4"><MetricList rows={historicalInspectorRows} /></div>
                  </div>
                  <p className="rounded-xl border border-dashed border-border bg-muted px-4 py-3 text-[10.5px] leading-5 text-muted-foreground">选择左侧具体模型后，可查看模型配置、最近 24 小时运行状态和端点可用性。</p>
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
