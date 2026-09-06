import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
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
import { useSchedulerTopology, type SchedulerEndpointTopology, type SchedulerTopologyResponse } from '@fluxeme/shared/src/api/scheduler';
import { bindingKey } from './routingFlowTopology';
import RoutingFlow from './RoutingFlow';
import type { RoutingFlowEndpointMeta, RoutingFlowNodeSelection } from './RoutingFlow';
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
  provider?: string;
  endpointId: number | null;
  endpointUrl: string;
  endpointEnabled: boolean;
  routingObserved: boolean;
  routingAvailable: boolean;
  probe: ProbeResult | null;
  channelRequests24h: number;
  channelSuccessRate24h?: number;
  channelP95Latency24h?: number;
  circuitEnabled: boolean;
  circuitOk: boolean;
  circuitState: string;
};

type RecentDecision = {
  id: string;
  timestamp: string;
  model: string;
  channelId: string;
  endpointId: number | null;
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

const RANGE_OPTIONS: Array<{ key: RangeKey; short: string; i18n: string }> = [
  { key: '5m', short: '5M', i18n: 'flowControl.range5m' },
  { key: '15m', short: '15M', i18n: 'flowControl.range15m' },
  { key: '1h', short: '1H', i18n: 'flowControl.range1h' },
  { key: '6h', short: '6H', i18n: 'flowControl.range6h' },
  { key: '24h', short: '24H', i18n: 'flowControl.range24h' },
];

const RANGE_MINUTES: Record<RangeKey, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
};

const FLOW_TABS: Array<{ key: FlowTabKey; i18n: string }> = [
  { key: 'flow', i18n: 'flowControl.tabFlow' },
  { key: 'endpoint', i18n: 'flowControl.tabEndpoint' },
  { key: 'compare', i18n: 'flowControl.tabCompare' },
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

function formatTrendBucket(bucket: string, unit: 'minute' | 'hour', locale = 'zh-CN') {
  const date = new Date(bucket);
  if (Number.isNaN(date.getTime())) {
    return bucket;
  }
  if (unit === 'minute') {
    return date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit', hour12: false });
  }
  return date.toLocaleString(locale, {
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

function inferRangeLabel(start: string | undefined, end: string | undefined, t: (key: string) => string, locale: string) {
  if (!start || !end) return t('flowControl.currentRange');
  const startDate = new Date(start);
  const endDate = new Date(end);
  if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) return t('flowControl.currentRange');
  const minutes = Math.round((endDate.getTime() - startDate.getTime()) / 60_000);
  const preset = Object.entries(RANGE_MINUTES).find(([, value]) => value === minutes)?.[0] as RangeKey | undefined;
  if (preset) {
    return t(RANGE_OPTIONS.find((option) => option.key === preset)?.i18n ?? 'flowControl.currentRange');
  }
  return `${startDate.toLocaleString(locale, { hour12: false })} ~ ${endDate.toLocaleString(locale, { hour12: false })}`;
}

function formatContextLength(value: number | null | undefined) {
  if (!value) return '—';
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return formatNumber(value);
}

function formatDateTime(value?: string | null, locale = 'zh-CN') {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(locale, { hour12: false });
}

function endpointRoutingLabel(row: EndpointStatusRow, t: (key: string) => string) {
  if (!row.routingObserved) return t('flowControl.unobserved');
  return row.routingAvailable ? t('flowControl.routable') : t('flowControl.notRoutable');
}

function endpointProbeLabel(probe: ProbeResult | null, t: (key: string, options?: Record<string, unknown>) => string) {
  if (!probe) return t('flowControl.unprobed');
  if (probe.success) return `${t('flowControl.success')} · ${formatNumber(probe.latency_ms)}ms`;
  return t('flowControl.failed');
}

function modelHealthPresentation(status: ModelHealthStatus, t: (key: string) => string) {
  switch (status) {
    case 'healthy':
      return { label: t('flowControl.healthy'), dot: 'bg-chart-2', badge: 'bg-chart-2/15 text-chart-2' };
    case 'degraded':
      return { label: t('flowControl.degraded'), dot: 'bg-sidebar-primary', badge: 'bg-sidebar-primary/15 text-sidebar-primary' };
    case 'unavailable':
      return { label: t('flowControl.unavailable'), dot: 'bg-destructive', badge: 'bg-destructive/15 text-destructive' };
    case 'unknown':
      return { label: t('flowControl.unknownHealth'), dot: 'bg-muted-foreground', badge: 'bg-muted text-muted-foreground' };
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
  const brokenCircuitChannels = channels.filter((channel) => channel.circuit_enabled && !channel.circuit_ok).length;

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
  sampleLabel,
}: {
  title: string;
  status: string;
  tone: 'orange' | 'cyan';
  metrics: FlowMetricsPercentiles;
  sampleLabel: string;
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
      <div className="mt-3 text-[10px] text-muted-foreground">{sampleLabel}: {formatNumber(metrics.sample_count)}</div>
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

// ── Scheduler policy overview ───────────────────────────────────────
// This is a read-only view of configured/eligible weights and observed traffic.
// Shares are neutral distribution metrics; they do not indicate health.

type TopoDecided = {
  model: string;
  channel_id: string;
  endpoint_id?: number | null;
};

function resolveTopoEvent(
  topology: SchedulerTopologyResponse | undefined,
  ev: { model: string; channel_id: string; endpoint_id?: number | null },
): TopoDecided {
  const channel = topology?.bindings.find((binding) => binding.channel_id === ev.channel_id);
  if (!channel) return { model: ev.model, channel_id: ev.channel_id, endpoint_id: ev.endpoint_id };
  const endpoint = channel.endpoints.find((ep) => ep.endpoint_id === ev.endpoint_id);
  return {
    model: ev.model,
    channel_id: channel.channel_name || ev.channel_id,
    endpoint_id: endpoint ? endpoint.endpoint_id : ev.endpoint_id,
  };
}

function useRecentDecisions(modelId: string, modelName?: string) {
  const [events, setEvents] = useState<RecentDecision[]>([]);
  const [live, setLive] = useState(false);
  const modelIdRef = useRef(modelId);
  const modelNameRef = useRef(modelName);
  modelIdRef.current = modelId;
  modelNameRef.current = modelName;

  useEffect(() => {
    setEvents([]);
    setLive(false);
    let socket: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;

    function connect() {
      const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
      socket = new WebSocket(`${proto}://${window.location.host}/api/health/ws`);

      socket.onopen = () => setLive(true);
      socket.onmessage = (message) => {
        if (closed) return;
        let event: { type?: string; model?: string; request_id?: string; channel_id?: string; endpoint_id?: number | null; timestamp?: string };
        try {
          event = JSON.parse(message.data as string);
        } catch {
          return;
        }
        if (event.type !== 'route_decided') return;
        if (!event || typeof event.model !== 'string' || typeof event.channel_id !== 'string') return;
        // WebSocket events use the model name; the selected model control uses
        // its id. Match only when the parent has resolved the name.
        if (modelIdRef.current !== 'all' && (!modelNameRef.current || event.model !== modelNameRef.current)) return;
        const model = event.model;
        const channelId = event.channel_id;
        const endpointId = event.endpoint_id ?? null;
        const timestamp = event.timestamp ?? new Date().toISOString();
        const requestId = event.request_id ?? `${timestamp}-${Math.random().toString(36).slice(2)}`;
        setEvents((prev) => [
          // Phase 1 identity: request_id + timestamp + endpoint_id. Phase 2
          // adds attempt once per-attempt events exist.
          { id: `${requestId}:${timestamp}:${endpointId ?? 'none'}`, timestamp, model, channelId, endpointId },
          ...prev,
        ].slice(0, 12));
      };
      socket.onclose = () => {
        setLive(false);
        if (!closed) retry = setTimeout(connect, 3000);
      };
      socket.onerror = () => {
        try {
          socket?.close();
        } catch {
          /* noop */
        }
      };
    }

    connect();
    return () => {
      closed = true;
      if (retry) clearTimeout(retry);
      try {
        socket?.close();
      } catch {
        /* noop */
      }
    };
  }, [modelId]);

  return { events, live };
}

function SchedulingPolicyCard({
  label,
  share,
  requests24h,
  observedShare24h,
  status,
  statusTone,
  statusDetail,
  footer,
}: {
  label: string;
  share: number | null;
  requests24h: number;
  observedShare24h: number | null;
  status: string;
  statusTone: 'healthy' | 'degraded' | 'open' | 'disabled';
  statusDetail?: string;
  footer?: ReactNode;
}) {
  const pctLabel = share == null ? '—' : `${(share * 100).toFixed(1)}%`;
  const pctWidth = share == null ? 0 : Math.min(100, share * 100);
  const gapLabel = share != null && observedShare24h != null
    ? `${share >= observedShare24h ? '+' : ''}${(share * 100 - observedShare24h * 100).toFixed(1)}%`
    : '—';
  const gapTone = 'text-muted-foreground';
  const dotClass = statusTone === 'healthy'
    ? 'bg-emerald-500'
    : statusTone === 'degraded'
      ? 'bg-amber-500'
      : statusTone === 'open'
        ? 'bg-red-500'
        : 'bg-muted-foreground/40';
  const toneLabel = 'text-muted-foreground';

  return (
    <div className="rounded-xl border border-secondary bg-card p-3">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-[10.5px] font-semibold text-muted-foreground">{label}</span>
        <span className="inline-flex items-center gap-1.5 text-[10px] text-muted-foreground">
          <i aria-hidden="true" className={`h-2 w-2 rounded-full ${dotClass}`} />
          {status}
        </span>
      </div>
      <div className="mt-2.5 flex items-baseline gap-1.5">
        <span className={`text-2xl font-semibold tracking-[-0.04em] tabular-nums ${toneLabel}`}>{pctLabel}</span>
        <span className="text-[10px] text-muted-foreground">/ {formatNumber(requests24h)} req 24h</span>
      </div>
      <div className="mt-2.5 flex items-center gap-2">
        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-secondary">
          <div className="h-full rounded-full bg-muted-foreground/35" style={{ width: `${pctWidth}%` }} />
        </div>
        <span className={`text-[10px] tabular-nums ${gapTone}`}>{gapLabel}</span>
      </div>
      {statusDetail ? (
        <p className="mt-2 text-[10px] leading-4 text-muted-foreground">{statusDetail}</p>
      ) : null}
      {footer ? <div className="mt-3 border-t border-secondary pt-2">{footer}</div> : null}
    </div>
  );
}

function RecentDecisionCard({
  decision,
  topology,
  locale,
  onHover,
  onClick,
}: {
  decision: RecentDecision;
  topology: SchedulerTopologyResponse | undefined;
  locale: string;
  onHover?: (node: string | null) => void;
  onClick?: () => void;
}) {
  const resolved = resolveTopoEvent(topology, {
    model: decision.model,
    channel_id: decision.channelId,
    endpoint_id: decision.endpointId,
  });
  const label = resolved.endpoint_id != null
    ? `${resolved.channel_id} · #${resolved.endpoint_id}`
    : resolved.channel_id;
  const [hover, setHover] = useState(false);

  return (
    <button
      type="button"
      onMouseEnter={() => { setHover(true); onHover?.(decision.channelId); }}
      onMouseLeave={() => { setHover(false); onHover?.(null); }}
      onFocus={() => onHover?.(decision.channelId)}
      onBlur={() => onHover?.(null)}
      onClick={onClick}
      className={`w-full rounded-lg border px-3 py-2 text-left transition ${
        hover ? 'border-accent bg-accent/60' : 'border-transparent hover:bg-muted'
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-[10.5px] font-medium text-foreground">{label}</span>
        <span className="text-[9.5px] text-muted-foreground">{formatDateTime(decision.timestamp, locale)}</span>
      </div>
      <div className="mt-0.5 truncate font-mono text-[9.5px] text-muted-foreground">{decision.model}</div>
    </button>
  );
}

export default function FlowTowerContent() {
  const { t, i18n } = useTranslation();
  const locale = i18n.language.startsWith('zh') ? 'zh-CN' : 'en-US';
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

  // Scheduler policy topology (read-only here) — same data SchedulerControl
  // renders for editing; this panel only visualizes configured vs observed share.
  const topologyQuery = useSchedulerTopology(selectedModelId === 'all' ? '' : selectedModelId);
  const [hoveredTopoNode, setHoveredTopoNode] = useState<string | null>(null);
  const [selectedTopoNode, setSelectedTopoNode] = useState<RoutingFlowNodeSelection | null>(null);
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
  const { events: recentDecisions, live: recentDecisionsLive } = useRecentDecisions(selectedModelId, modelParam);
  // Binding-level scheduling fields for the topology endpoint cards, keyed by
  // the topology node key (`<channelId>:id:<endpoint_id>`) so the same physical
  // endpoint bound to different channels never shares scheduling metadata.
  // P95 is channel-level (endpoint percentiles do not exist in the current
  // aggregation) and labeled as such.
  const endpointMeta = useMemo(() => {
    const meta: Record<string, RoutingFlowEndpointMeta> = {};
    const topo = topologyQuery.data;
    if (!topo) return meta;
    const modelHealth = selectedCatalogModel
      ? healthByModelId.get(selectedCatalogModel.config.id)
      : undefined;
    const channelP95 = new Map(
      (modelHealth?.channels ?? []).map((ch) => [ch.channel_id, ch.p95_latency_ms]),
    );
    for (const binding of topo.bindings) {
      const p95 = channelP95.get(binding.channel_id) ?? null;
      for (const ep of binding.endpoints) {
        meta[`${binding.channel_id}:id:${ep.endpoint_id}`] = {
          weight: ep.weight,
          timeoutSecs: ep.timeout_secs,
          maxTokens: ep.max_tokens,
          p95Ms: p95,
          routeEligible: ep.routing_available,
          breakerState: ep.circuit_state,
        };
      }
    }
    return meta;
  }, [healthByModelId, selectedCatalogModel, topologyQuery.data]);
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
  const displayedRangeLabel = inferRangeLabel(displayedRange?.start, displayedRange?.end, t, locale);
  const displayedModelLabel = displayedRangeModel ?? t('flowControl.allModels');
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
    ? t('flowControl.unavailableReason', { reason: realtime?.queue.reason ?? t('flowControl.pendingBackend') })
    : t('flowControl.status', { status: realtime?.queue.status });

  const publishedModelNames = useMemo(
    () => new Set(catalogModels.map((model) => model.config.name)),
    [catalogModels],
  );
  const shareRows: FlowMetricsModelShare[] = (historical?.model_share ?? [])
    .filter((row) => publishedModelNames.has(row.model));
  const ipRows = useMemo(
    () => formatIpRows(historical?.client_ips ?? []),
    [historical?.client_ips],
  );
  const trendData = useMemo(() => {
    const trend = historical?.trend;
    if (!trend) return [];
    return trend.buckets.map((bucket, index) => ({
      bucket,
      label: formatTrendBucket(bucket, trend.bucket_unit, locale),
      success_completed: trend.success_completed[index] ?? 0,
      failed_completed: trend.failed_completed[index] ?? 0,
    }));
  }, [historical?.trend]);

  const historicalInspectorRows: MetricRow[] = [
    { label: t('flowControl.rangeLabel'), value: displayedRangeLabel },
    { label: t('flowControl.modelFilter'), value: displayedModelLabel },
    { label: t('flowControl.completed'), value: formatNumber(totalCompleted) },
    { label: t('flowControl.successCompleted'), value: formatNumber(historical?.success_completed) },
    { label: t('flowControl.failedCompleted'), value: formatNumber(historical?.failed_completed) },
    { label: t('flowControl.successRate'), value: formatPercent(successRate) },
    { label: t('flowControl.p99Latency'), value: historical?.latency_ms.p99 != null ? `${formatPercentile(historical.latency_ms.p99)} ms` : '—' },
    { label: 'TTFT P99', value: historical?.ttft_ms.p99 != null ? `${formatPercentile(historical.ttft_ms.p99)} ms` : '—' },
  ];

  const endpointRows = useMemo(() => {
    if (!selectedCatalogModel) return [] as EndpointStatusRow[];

    const healthChannels = new Map((selectedCatalogModel.health?.channels ?? []).map((channel) => [channel.channel_id, channel]));
    const probeRows = (probeResultsQuery.data ?? []).filter((row) => row.model_id === selectedCatalogModel.config.id);
    // Binding-level eligibility for the selected model comes from the scheduler
    // topology (routing_available / circuit_state), never from endpoint_id-only
    // aggregates. Build a (channel_id, endpoint_id) lookup.
    const topoByBinding = new Map<string, SchedulerEndpointTopology>();
    for (const binding of topologyQuery.data?.bindings ?? []) {
      for (const ep of binding.endpoints) {
        topoByBinding.set(bindingKey(binding.channel_id, ep.endpoint_id), ep);
      }
    }

    return selectedCatalogModel.config.channels.flatMap((binding) => {
      const channel = channelById.get(binding.channel_id);
      if (!channel?.enabled) return [];
      const channelHealth = healthChannels.get(binding.channel_id);
      return channel.endpoints
        .filter((endpoint) => endpoint.enabled !== false)
        .map((endpoint) => {
          const endpointHealth = channelHealth?.endpoints.find((item) => item.endpoint_id === endpoint.id);
          const topo = endpoint.id != null ? topoByBinding.get(bindingKey(binding.channel_id, endpoint.id)) : undefined;
          const probe = probeRows.find((row) => row.channel_id === binding.channel_id && row.endpoint_url === endpoint.url) ?? null;
          const circuitState = topo?.circuit_state ?? 'unknown';
          return {
            channelId: binding.channel_id,
            channelName: channel.name || binding.channel_id,
            provider: channel.provider,
            endpointId: endpoint.id ?? null,
            endpointUrl: endpoint.url,
            endpointEnabled: endpoint.enabled !== false,
            // Binding-level availability: topology routing_available is the
            // selected model's real eligibility; routing_health is channel-level.
            routingObserved: topo != null || Boolean(endpointHealth),
            routingAvailable: topo != null ? topo.routing_available : (endpointHealth?.available ?? false),
            probe,
            channelRequests24h: channelHealth?.requests ?? 0,
            channelSuccessRate24h: channelHealth?.requests ? channelHealth.success_rate * 100 : undefined,
            channelP95Latency24h: channelHealth?.requests ? channelHealth.p95_latency_ms : undefined,
            // Circuit comes from real circuit_state (topology) when available,
            // falling back to channel-level circuit info from routing_health.
            circuitState,
            circuitEnabled: topo != null ? circuitState !== 'disabled' : (channelHealth?.circuit_enabled ?? false),
            circuitOk: topo != null ? circuitState === 'closed' : (channelHealth?.circuit_ok ?? false),
          } satisfies EndpointStatusRow;
        });
    });
  }, [channelById, probeResultsQuery.data, selectedCatalogModel, topologyQuery.data]);

  const endpointSummaryRows: MetricRow[] = useMemo(() => {
    const enabled = endpointRows.length;
    const observed = endpointRows.filter((row) => row.routingObserved).length;
    const available = endpointRows.filter((row) => row.routingAvailable).length;
    const probeSuccess = endpointRows.filter((row) => row.probe?.success).length;
    const probeFail = endpointRows.filter((row) => row.probe && !row.probe.success).length;
    const probeUnknown = endpointRows.filter((row) => !row.probe).length;

    return [
      { label: t('flowControl.enabledEndpoints'), value: formatNumber(enabled) },
      { label: t('flowControl.observedEndpoints'), value: formatNumber(observed) },
      { label: t('flowControl.availableEndpoints'), value: formatNumber(available) },
      { label: t('flowControl.probeSuccess'), value: formatNumber(probeSuccess) },
      { label: t('flowControl.probeFail'), value: formatNumber(probeFail) },
      { label: t('flowControl.probeUnknown'), value: formatNumber(probeUnknown) },
    ];
  }, [endpointRows]);

  const compareModelShareRows = (compareMetricsQuery.data?.historical.model_share ?? [])
    .filter((row) => publishedModelNames.has(row.model));
  const compareModelShareMap = useMemo(
    () => new Map(compareModelShareRows.map((row) => [row.model, row])),
    [compareModelShareRows],
  );
  const unmatchedCompareModels = useMemo(
    () => compareModelShareRows.filter((row) => !publishedModelNames.has(row.model)),
    [compareModelShareRows, publishedModelNames],
  );

  const compareRows = useMemo(() => {
    const rangeMetricsAvailable = !compareMetricsQuery.isError && compareMetricsQuery.data != null;
    return catalogModels.map((model) => {
      const health = modelHealthPresentation(model.status, t);
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
            <h2 className="text-lg font-semibold text-foreground">{t('flowControl.title')}</h2>
            <p className="mt-1 text-sm text-muted-foreground">{t('flowControl.subtitle')}</p>
          </div>
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-muted-foreground">
              {t('flowControl.autoRefresh')}
            </div>
            <div className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-muted-foreground">
              {t('flowControl.lastUpdated', { time: lastUpdatedLabel })}
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
                    {t(option.i18n)}
                  </button>
                ))}
              </div>
              <select
                aria-label={t('flowControl.modelFilterAria')}
                className="h-9 rounded-lg border border-border bg-background px-3 text-xs text-muted-foreground"
                value={selectedModelId}
                onChange={(event) => {
                  setSelectedModelId(event.target.value);
                  setQueryNowMs(Date.now());
                }}
              >
                <option value="all">{t('flowControl.allModels')}</option>
                {catalogModels.map((model) => (
                  <option key={model.config.id} value={model.config.id}>
                    {model.config.name}
                  </option>
                ))}
              </select>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <select
                aria-label={t('flowControl.channelFilterAria')}
                className="h-9 rounded-lg border border-border bg-muted px-3 text-xs text-muted-foreground"
                value="pending"
                disabled
              >
                <option value="pending">{t('flowControl.channelPending')}</option>
              </select>
              <button
                type="button"
                onClick={() => setQueryNowMs(Date.now())}
                className="inline-flex h-9 items-center rounded-lg border border-border bg-background px-3 text-xs text-muted-foreground transition hover:bg-muted"
              >
                ↻ {t('flowControl.refresh')}
              </button>
            </div>
          </div>

          <section className="grid gap-3 md:grid-cols-2 2xl:grid-cols-6">
            <KpiCard
              label={t('flowControl.inFlight')}
              badge="LIVE"
              value={formatNumber(realtime?.in_flight)}
              tone="blue"
              subtext={t('flowControl.inFlightSub')}
            />
            <KpiCard
              label={t('flowControl.generating')}
              badge="LIVE"
              value={formatNumber(realtime?.upstream_generating)}
              tone="cyan"
              subtext={t('flowControl.generatingSub')}
            />
            <KpiCard
              label={t('flowControl.outputting')}
              badge="LIVE"
              value={formatNumber(realtime?.upstream_outputting)}
              tone="green"
              subtext={t('flowControl.outputtingSub')}
            />
            <KpiCard
              label={t('flowControl.queued')}
              badge="PENDING"
              value={queueValue}
              tone="yellow"
              subtext={queueSubtext}
            />
            <KpiCard
              label={t('flowControl.successCompleted')}
              badge={displayedRangeLabel}
              value={formatNumber(historical?.success_completed)}
              tone="green"
              subtext={<>{t('flowControl.successRate')} <span className="font-semibold text-chart-2">{formatPercent(successRate)}</span></>}
            />
            <KpiCard
              label={t('flowControl.failedCompleted')}
              badge={displayedRangeLabel}
              value={formatNumber(historical?.failed_completed)}
              tone="red"
              subtext={<>{t('flowControl.errorRate')} <span className="font-semibold text-destructive">{formatPercent(failureRate)}</span></>}
            />
          </section>

          <section className="grid gap-4 2xl:grid-cols-[minmax(0,1.2fr)_minmax(380px,0.8fr)]">
            <div className="grid gap-4">
              <Panel
                title={t('flowControl.trendTitle')}
                subtitle={t('flowControl.rangeTrend', { range: displayedRangeLabel, unit: historical?.trend.bucket_unit === 'minute' ? t('flowControl.byMinute') : t('flowControl.byHour') })}
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
                          formatter={(value, name) => [formatNumber(Number(value)), name === 'success_completed' ? t('flowControl.completionSeries') : t('flowControl.failureSeries')]}
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
                title={t('flowControl.clientIpTitle')}
                subtitle={t('flowControl.requestSources', { range: displayedRangeLabel })}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-muted-foreground">
                    {t('flowControl.byRequestCount')}
                  </div>
                }
              >
                {ipRows.length > 0 ? (
                  <div className="overflow-x-auto">
                    <table className="w-full min-w-[520px] border-collapse">
                      <thead>
                        <tr>
                          <th className="pb-2 text-left text-[10px] font-semibold text-muted-foreground">{t('flowControl.clientIp')}</th>
                          <th className="pb-2 text-left text-[10px] font-semibold text-muted-foreground">{t('flowControl.requestShare')}</th>
                          <th className="pb-2 text-right text-[10px] font-semibold text-muted-foreground">{t('flowControl.requests')}</th>
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
                  <PlaceholderCard title={t('flowControl.noClientData')} description={t('flowControl.noClientDataDesc')} />
                )}
              </Panel>
            </div>

            <div className="grid gap-4">
              <Panel
                title={t('flowControl.modelShareTitle')}
                subtitle={t('flowControl.modelShareRange', { range: displayedRangeLabel })}
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
                  <PlaceholderCard title={t('flowControl.noModelShareData')} description={t('flowControl.noModelShareDataDesc')} />
                )}
              </Panel>

              <Panel
                title={t('flowControl.latencyTitle')}
                subtitle={t('flowControl.allModelSummary', { range: displayedRangeLabel })}
                right={
                  <div className="inline-flex h-7 items-center rounded-md border border-border bg-background px-2.5 text-[11px] text-muted-foreground">
                    ms
                  </div>
                }
              >
                <div className="grid gap-3">
                  <PercentileBlock
                    title={t('flowControl.requestLatency')}
                    status={t('flowControl.usageEvents')}
                    tone="orange"
                    metrics={historical?.latency_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                    sampleLabel={t('flowControl.sampleCountLabel')}
                  />
                  <PercentileBlock
                    title="TTFT"
                    status={t('flowControl.ttftSamples')}
                    tone="cyan"
                    metrics={historical?.ttft_ms ?? { p50: null, p90: null, p99: null, sample_count: 0 }}
                    sampleLabel={t('flowControl.sampleCountLabel')}
                  />
                </div>
              </Panel>
            </div>
          </section>

          <section className="grid gap-4 xl:grid-cols-[240px_minmax(0,1fr)] 2xl:grid-cols-[240px_minmax(0,1fr)_310px]">
            <Panel
              title={t('flowControl.catalogTitle')}
              subtitle={t('flowControl.catalogSummary', { count: catalogModels.length })}
              className="min-h-[580px]"
              right={routingHealthQuery.isFetching ? <span className="text-[10px] text-muted-foreground">{t('flowControl.updating')}</span> : null}
            >
              <div className="-mt-1">
                <div className="mb-3">
                  <input
                    aria-label={t('flowControl.searchModel')}
                    value={modelSearch}
                    onChange={(event) => setModelSearch(event.target.value)}
                    placeholder={t('flowControl.searchPlaceholder')}
                    className="h-9 w-full rounded-lg border border-border bg-muted px-3 text-[11px] text-muted-foreground outline-none placeholder:text-muted-foreground"
                  />
                </div>
                {routingHealthQuery.isError ? (
                  <button
                    type="button"
                    onClick={() => void routingHealthQuery.refetch()}
                    className="mb-3 w-full rounded-lg border border-dashed border-border px-3 py-2 text-[10px] text-muted-foreground hover:bg-muted"
                  >
                    {t('flowControl.retryRouting')}
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
                    <div className="text-[11px] font-semibold text-muted-foreground">{t('flowControl.allModels')}</div>
                    <div className="mt-2 text-[10px] text-muted-foreground">{t('flowControl.catalogSnapshot')}</div>
                  </button>
                  {visibleModels.length > 0 ? (
                    visibleModels.map((model) => {
                      const health = modelHealthPresentation(model.status, t);
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
                              <span>{t('flowControl.noRoutingData')}</span>
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
                      {t('flowControl.loadingModelList')}
                    </div>
                  ) : modelsQuery.isError ? (
                    <button
                      type="button"
                      onClick={() => void modelsQuery.refetch()}
                      className="w-full rounded-xl border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground hover:bg-muted"
                    >
                      {t('flowControl.modelListFailed')}
                    </button>
                  ) : (
                    <div className="rounded-xl border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground">
                      {t('flowControl.noModelsMatched')}
                    </div>
                  )}
                </div>
              </div>
            </Panel>

            <article className="overflow-hidden rounded-2xl border border-border bg-card shadow-sm">
              <div className="flex gap-4 border-b border-border px-4 pt-3" role="tablist" aria-label={t('flowControl.detailTabs')}>
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
                    {t(tab.i18n)}
                  </button>
                ))}
              </div>

              {activeTab === 'flow' ? (
                <div id="flowtower-panel-flow" role="tabpanel" aria-labelledby="flowtower-tab-flow" className="space-y-5 p-4">
                  {!selectedCatalogModel ? (
                    <PlaceholderCard
                      title={t('flowControl.selectModelFlow')}
                      description={t('flowControl.selectModelFlowDesc')}
                    />
                  ) : (
                    <RoutingFlow
                      embedded
                      modelName={selectedCatalogModel.config.name}
                      endpointMeta={endpointMeta}
                      onSelectNode={setSelectedTopoNode}
                      selectedNode={selectedTopoNode}
                    />
                  )}
                </div>
              ) : null}

              {activeTab === 'endpoint' ? (
                <div id="flowtower-panel-endpoint" role="tabpanel" aria-labelledby="flowtower-tab-endpoint" className="space-y-5 p-4">
                  {!selectedCatalogModel ? (
                    <PlaceholderCard
                      title={t('flowControl.selectModelEndpoint')}
                      description={t('flowControl.selectModelEndpointDesc')}
                    />
                  ) : channelsQuery.isError ? (
                    <button
                      type="button"
                      onClick={() => void channelsQuery.refetch()}
                      className="w-full rounded-xl border border-dashed border-border px-4 py-6 text-center text-[11px] text-muted-foreground hover:bg-muted"
                    >
                      {t('flowControl.endpointLoadFailed')}
                    </button>
                  ) : endpointRows.length === 0 ? (
                    <PlaceholderCard
                      title={t('flowControl.noEnabledEndpoint')}
                      description={t('flowControl.noEnabledEndpointDesc')}
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
                          {t('flowControl.healthDataFailed')}
                        </div>
                      ) : null}

                      {probeResultsQuery.isError ? (
                        <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                          {t('flowControl.probeDataFailed')}
                        </div>
                      ) : null}

                      <div className="overflow-x-auto rounded-xl border border-secondary bg-card">
                        <table className="w-full min-w-[980px] border-collapse">
                          <thead>
                            <tr>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.channel')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.endpoint')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.configuration')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.routingStatus')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.latestProbe')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.latestProbeTime')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('routingFlow.channelRequests24h')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.successRate24h')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('routingFlow.channelP9524h')}</th>
                              <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.circuit')}</th>
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
                                  <div>{row.endpointEnabled ? t('flowControl.enabled') : t('flowControl.disabled')}</div>
                                </td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{endpointRoutingLabel(row, t)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{endpointProbeLabel(row.probe, t)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">{formatDateTime(row.probe?.probed_at, locale)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatNumber(row.channelRequests24h)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{formatPercent(row.channelSuccessRate24h)}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] text-muted-foreground">{row.channelP95Latency24h == null ? '—' : `${formatNumber(Math.round(row.channelP95Latency24h))} ms`}</td>
                                <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground font-mono uppercase">{row.circuitState}</td>
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
                    {t('flowControl.compareHint')}
                  </div>
                  {compareMetricsQuery.isError ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      {t('flowControl.rangeDataFailed')}
                    </div>
                  ) : null}
                  {routingHealthQuery.isError ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      {t('flowControl.healthDataFailed')}
                    </div>
                  ) : null}
                  {unmatchedCompareModels.length > 0 ? (
                    <div className="rounded-xl border border-dashed border-border px-4 py-3 text-[10.5px] text-muted-foreground">
                      {t('flowControl.unmatchedModels', { count: unmatchedCompareModels.length })}
                    </div>
                  ) : null}
                  {compareRows.length > 0 ? (
                    <div className="overflow-x-auto rounded-xl border border-secondary bg-card">
                      <table className="w-full min-w-[980px] border-collapse">
                        <thead>
                          <tr>
                            <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.model')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-left text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.currentStatus')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.rangeRequests')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.rangeShare')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.routeRequests24h')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.routeSuccess24h')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.avgLatency24h')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.maxP9524h')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.availableEndpoint')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.brokenChannels')}</th>
                            <th className="border-b border-secondary px-3 py-2.5 text-right text-[10.5px] font-semibold text-muted-foreground">{t('flowControl.configuredChannels')}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {compareRows.map((row) => (
                            <tr key={row.id} className={row.selected ? 'bg-accent' : ''}>
                              <td className="border-b border-secondary px-3 py-2.5 text-[10.5px] text-muted-foreground">
                                <div className="font-semibold">{row.name}</div>
                                <div className="text-[10px] text-muted-foreground">{row.selected ? t('flowControl.selected') : '—'}</div>
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
                      title={t('flowControl.noCompareModels')}
                      description={t('flowControl.noCompareModelsDesc')}
                    />
                  )}
                </div>
              ) : null}
            </article>

            <Panel
              title={t('flowControl.inspector')}
              subtitle={selectedCatalogModel ? t('flowControl.inspectorSelected') : t('flowControl.inspectorAll')}
              className="min-h-[580px]"
            >
              {selectedCatalogModel ? (() => {
                const health = modelHealthPresentation(selectedCatalogModel.status, t);
                const categories = selectedCatalogModel.config.category?.split(',').map((item) => item.trim()).filter(Boolean) ?? [];
                const routingRows: MetricRow[] = [
                  { label: t('flowControl.routingStatus'), value: health.label },
                  { label: t('flowControl.availableEndpoint'), value: `${selectedCatalogModel.availableEndpoints} / ${selectedCatalogModel.enabledEndpoints}` },
                  { label: t('flowControl.observedEndpoints'), value: `${selectedCatalogModel.observedEndpoints} / ${selectedCatalogModel.enabledEndpoints}` },
                  { label: t('flowControl.routeRequests24h'), value: selectedCatalogModel.routedRequests24h == null ? '—' : formatNumber(selectedCatalogModel.routedRequests24h) },
                  { label: t('flowControl.successRate24h'), value: formatPercent(selectedCatalogModel.successRate24h == null ? null : selectedCatalogModel.successRate24h * 100) },
                  { label: t('flowControl.avgLatency24h'), value: selectedCatalogModel.averageLatency24h == null ? '—' : `${formatNumber(Math.round(selectedCatalogModel.averageLatency24h))} ms` },
                  { label: t('flowControl.maxP9524h'), value: selectedCatalogModel.highestChannelP95 == null ? '—' : `${formatNumber(Math.round(selectedCatalogModel.highestChannelP95))} ms` },
                  { label: t('flowControl.brokenChannels'), value: String(selectedCatalogModel.brokenCircuitChannels) },
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
                        <span className="rounded bg-card px-2 py-1 text-[10px] text-muted-foreground">{selectedCatalogModel.config.published ? t('flowControl.published') : t('flowControl.unpublished')}</span>
                        {categories.map((category) => <span key={category} className="rounded bg-card px-2 py-1 text-[10px] text-muted-foreground">{category}</span>)}
                      </div>
                      <div className="mt-4 grid grid-cols-[1fr_auto] gap-x-4 gap-y-2 text-[10.5px]">
                        <span className="text-muted-foreground">{t('flowControl.modelPattern')}</span><span className="max-w-[180px] truncate text-right text-muted-foreground">{selectedCatalogModel.config.model_pattern}</span>
                        <span className="text-muted-foreground">{t('flowControl.contextLength')}</span><span className="text-right text-muted-foreground">{formatContextLength(selectedCatalogModel.config.context_length)}</span>
                        <span className="text-muted-foreground">{t('flowControl.configuredChannels')}</span><span className="text-right text-muted-foreground">{selectedCatalogModel.config.channels.length}</span>
                      </div>
                    </div>

                    {selectedTopoNode?.channelId ? (() => {
                      const binding = topologyQuery.data?.bindings.find((b) => b.channel_id === selectedTopoNode.channelId);
                      const channelHealth = selectedCatalogModel.health?.channels.find((c) => c.channel_id === selectedTopoNode.channelId);
                      const channelP95 = channelHealth && channelHealth.requests > 0
                        ? `${formatNumber(Math.round(channelHealth.p95_latency_ms))} ms`
                        : '—';
                      const nodeRows: MetricRow[] = [];
                      let nodeTitle = selectedTopoNode.channelId;
                      if (selectedTopoNode.endpointKey != null && binding) {
                        const ep = binding.endpoints.find((e) => e.endpoint_id === selectedTopoNode.endpointId);
                        nodeTitle = ep?.url ?? `${selectedTopoNode.channelId} · #${selectedTopoNode.endpointId}`;
                        const probe = ep
                          ? probeResultsQuery.data?.find((p) => p.channel_id === selectedTopoNode.channelId && p.endpoint_url === ep.url)
                          : undefined;
                        if (ep) {
                          nodeRows.push({ label: t('routingFlow.endpointBreaker'), value: ep.circuit_state.toUpperCase() });
                          nodeRows.push({ label: t('flowControl.routingStatus'), value: ep.routing_available ? t('flowControl.routable') : t('flowControl.notRoutable') });
                          nodeRows.push({ label: t('routingFlow.endpointWeight'), value: String(ep.weight) });
                          nodeRows.push({ label: t('routingFlow.endpointTimeout'), value: ep.timeout_secs != null ? `${ep.timeout_secs}s` : '—' });
                          nodeRows.push({ label: t('routingFlow.endpointMaxTokens'), value: ep.max_tokens != null ? String(ep.max_tokens) : '—' });
                          nodeRows.push({ label: t('flowControl.latestProbe'), value: probe ? (probe.success ? t('flowControl.success') : t('flowControl.failed')) : t('flowControl.unprobed') });
                          nodeRows.push({ label: t('routingFlow.endpointChannelP95Short'), value: channelP95 });
                        }
                      } else if (binding) {
                        const eligible = binding.endpoints.filter((ep) => ep.routing_available).length;
                        nodeRows.push({ label: t('flowControl.requests24h'), value: formatNumber(binding.request_count_24h) });
                        nodeRows.push({ label: t('flowControl.requestShare'), value: binding.observed_model_share_24h == null ? '—' : formatPercent(binding.observed_model_share_24h * 100) });
                        nodeRows.push({ label: t('flowControl.availableEndpoint'), value: `${eligible} / ${binding.endpoints.length}` });
                        nodeRows.push({ label: t('flowControl.routingStatus'), value: binding.routing_state === 'available' ? t('flowControl.routable') : t('flowControl.notRoutable') });
                        nodeRows.push({ label: t('routingFlow.routingReason'), value: binding.routing_reason || '—' });
                        nodeRows.push({ label: t('routingFlow.endpointChannelP95Short'), value: channelP95 });
                      }
                      return (
                        <div className="rounded-xl border border-accent bg-card p-4">
                          <div className="mb-2 flex items-center justify-between gap-2">
                            <div className="text-[11px] font-semibold text-foreground">{t('routingFlow.inspectorSelectedNode')}</div>
                            <button
                              type="button"
                              onClick={() => setSelectedTopoNode(null)}
                              className="rounded px-1.5 text-[10px] text-muted-foreground transition hover:bg-muted hover:text-foreground"
                              aria-label={t('flowControl.refresh')}
                            >×</button>
                          </div>
                          <div className="mb-3 truncate font-mono text-[10.5px] text-foreground" title={nodeTitle}>{nodeTitle}</div>
                          {nodeRows.length > 0 ? <MetricList rows={nodeRows} /> : null}
                          <p className="mt-3 border-t border-secondary pt-2 text-[10px] leading-4 text-muted-foreground">{t('routingFlow.inspectorDecisionPlaceholder')}</p>
                        </div>
                      );
                    })() : null}

                    <div className="rounded-xl border border-secondary bg-card p-4">
                      <div className="mb-3 flex items-center justify-between gap-2">
                        <div className="text-[11px] font-semibold text-muted-foreground">{t('flowControl.modelStatus')}</div>
                        {routingHealthQuery.isFetching ? <span className="text-[10px] text-muted-foreground">{t('flowControl.updating')}</span> : null}
                      </div>
                      {routingHealthQuery.isError ? (
                        <button type="button" onClick={() => void routingHealthQuery.refetch()} className="text-[10.5px] text-muted-foreground underline">
                          {t('flowControl.retryRouting')}
                        </button>
                      ) : selectedCatalogModel.health ? (
                        <MetricList rows={routingRows} />
                      ) : (
                        <p className="text-[10.5px] leading-5 text-muted-foreground">{t('flowControl.noHealthDetail')}</p>
                      )}
                      {selectedCatalogModel.config.channels.length > 0 ? (
                        <div className="mt-3 border-t border-secondary pt-3">
                          <div className="mb-2 text-[10px] text-muted-foreground">{t('flowControl.channelBindings')}</div>
                          <div className="space-y-1.5">
                            {selectedCatalogModel.config.channels.map((channel) => (
                              <div key={channel.channel_id} className="flex justify-between gap-3 text-[10px] text-muted-foreground">
                                <span className="truncate">{channel.channel_id}{channel.upstream_model ? ` → ${channel.upstream_model}` : ''}</span>
                              </div>
                            ))}
                          </div>
                        </div>
                      ) : null}
                    </div>

                    <div className="rounded-xl border border-secondary bg-card p-4">
                      <div className="mb-3 text-[11px] font-semibold text-muted-foreground">{t('flowControl.metricsRange')}</div>
                      <MetricList rows={historicalInspectorRows} />
                    </div>
                  </div>
                );
              })() : (
                <div className="space-y-4">
                  <div className="rounded-xl border border-border bg-accent p-4">
                    <div className="flex items-center justify-between gap-2">
                      <strong className="text-sm text-foreground">{t('flowControl.allModels')}</strong>
                      <span className="rounded-md bg-accent px-2 py-1 text-[10px] font-semibold text-accent-foreground">{t('flowControl.aggregateView')}</span>
                    </div>
                    <div className="mt-4"><MetricList rows={historicalInspectorRows} /></div>
                  </div>
                  <p className="rounded-xl border border-dashed border-border bg-muted px-4 py-3 text-[10.5px] leading-5 text-muted-foreground">{t('flowControl.selectModelHint')}</p>
                </div>
              )}

              <div className="mt-4 border-t border-border pt-4">
                <div className="mb-3 text-[11px] font-semibold text-muted-foreground">{t('routingFlow.routingExplanation')}</div>
                <PlaceholderCard
                  title={t('routingFlow.inspectorDecisionPlaceholder')}
                  description={t('routingFlow.candidateUnavailable')}
                />
              </div>

              <div className="mt-4 border-t border-border pt-4">
                <div className="mb-3 flex items-center justify-between gap-2">
                  <div className="text-[11px] font-semibold text-muted-foreground">{t('routingFlow.inspectorDecision')}</div>
                  {topologyQuery.isFetching ? <span className="text-[10px] text-muted-foreground">{t('flowControl.updating')}</span> : null}
                </div>
                {topologyQuery.isError ? (
                  <button
                    type="button"
                    onClick={() => void topologyQuery.refetch()}
                    className="mb-3 w-full rounded-lg border border-dashed border-border px-3 py-2 text-[10px] text-muted-foreground hover:bg-muted"
                  >
                    {t('flowControl.retryRouting')}
                  </button>
                ) : topologyQuery.data ? (
                  (() => {
                    const topology = topologyQuery.data;
                    const eligibleTotal = topology.eligible_total_weight;
                    return (
                      <div className="space-y-2.5">
                        {topology.bindings.map((binding) => {
                          const share = eligibleTotal > 0 ? binding.eligible_total_weight / eligibleTotal : null;
                          const liveEndpoints = binding.endpoints.filter((ep) => ep.routing_available);
                          let statusTone: 'healthy' | 'degraded' | 'open' | 'disabled' = 'disabled';
                          let status = t('flowControl.disabled');
                          if (binding.channel_enabled) {
                            // Routing state comes from the backend routing_state —
                            // never inferred as "circuit open" from a zero weight.
                            if (binding.routing_state === 'available') {
                              statusTone = 'healthy';
                              status = t('flowControl.routable');
                            } else {
                              statusTone = 'open';
                              status = t('flowControl.notRoutable');
                            }
                          }
                          const statusDetail = binding.routing_reason || (
                            liveEndpoints.length === 0
                              ? t('flowControl.noRoutingData')
                              : `${liveEndpoints.length}/${binding.endpoints.length} endpoints eligible`
                          );
                          const endpointFooter = (
                            <div className="space-y-1">
                              {binding.endpoints.map((ep) => (
                                <div key={ep.endpoint_id} className="flex items-center justify-between gap-2 text-[9.5px] text-muted-foreground">
                                  <span
                                    className={`truncate font-mono ${hoveredTopoNode === binding.channel_id ? 'text-foreground' : ''}`}
                                    title={ep.url}
                                  >
                                    {ep.url || `#${ep.endpoint_id}`}
                                  </span>
                                  <span className="flex shrink-0 items-center gap-2">
                                    <span className={ep.routing_available ? 'text-chart-2' : 'text-destructive'}>
                                      {ep.routing_available ? t('flowControl.routable') : t('flowControl.notRoutable')}
                                    </span>
                                    <span className="rounded bg-muted px-1.5 py-0.5 font-mono text-[9px] uppercase">
                                      {ep.circuit_state}
                                    </span>
                                  </span>
                                </div>
                              ))}
                            </div>
                          );
                          return (
                            <SchedulingPolicyCard
                              key={binding.channel_id}
                              label={binding.channel_name || binding.channel_id}
                              share={share}
                              requests24h={binding.request_count_24h}
                              observedShare24h={binding.observed_model_share_24h}
                              status={status}
                              statusTone={statusTone}
                              statusDetail={statusDetail}
                              footer={endpointFooter}
                            />
                          );
                        })}
                      </div>
                    );
                  })()
                ) : (
                  <p className="text-[10.5px] leading-5 text-muted-foreground">{t('flowControl.selectModelHint')}</p>
                )}
              </div>

              <div className="mt-4 border-t border-border pt-4">
                <div className="mb-3 flex items-center justify-between gap-2">
                  <div className="text-[11px] font-semibold text-muted-foreground">{t('routingFlow.recentEvents')}</div>
                  <span className="inline-flex items-center gap-1.5 text-[10px] text-muted-foreground">
                    <i aria-hidden="true" className={`h-1.5 w-1.5 rounded-full ${recentDecisionsLive ? 'bg-chart-2' : 'bg-muted-foreground/40'}`} />
                    {recentDecisionsLive ? 'LIVE' : '—'}
                  </span>
                </div>
                {recentDecisions.length > 0 ? (
                  <div className="flex flex-col gap-1">
                    {recentDecisions.map((decision) => (
                      <RecentDecisionCard
                        key={decision.id}
                        decision={decision}
                        topology={topologyQuery.data}
                        locale={locale}
                        onHover={setHoveredTopoNode}
                        onClick={() => setSelectedTopoNode({ model: decision.model, channelId: decision.channelId, endpointId: decision.endpointId, endpointKey: decision.endpointId != null ? `id:${decision.endpointId}` : undefined })}
                      />
                    ))}
                  </div>
                ) : (
                  <PlaceholderCard
                    title={t('routingFlow.recentEventsEmpty')}
                    description={t('routingFlow.inspectorDecisionPlaceholder')}
                  />
                )}
              </div>

              <div className="mt-4 border-t border-border pt-4">
                <PlaceholderCard
                  title={t('flowControl.incidentsPending')}
                  description={t('flowControl.incidentsPendingDesc')}
                />
              </div>
            </Panel>
          </section>
        </div>
      </section>
    </div>
  );
}
