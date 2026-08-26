import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { Search } from 'lucide-react';
import { cn } from '@/lib/utils';
import { api } from '@/api/client';
import { usePublicModels } from '@/api/models';
import { useChannels } from '@/api/channels';
import { useProbeResults } from '@/api/probe';
import { useUsageFunnel, useUsageAggregate, useModelActivity } from '@/api/usage';
import { useDashboardAggregations } from '@/api/dashboard';
import { fetchRoutingFlowSnapshot, useRoutingHealth } from '@/api/routing';
import type { Model, ModelActivity, ProbeResult } from '@/types';
import type { RoutingHealthChannel, RoutingHealthModel, RoutingHealthResponse } from '@/api/routing';

// ── formatters ─────────────────────────────────────────────────────

function fmtCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}
function fmtTokens(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}
function fmtLat(ms: number) {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  return `${Math.round(ms)}ms`;
}
function fmtHour(iso: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${String(d.getHours()).padStart(2, '0')}:00`;
}

type Health = 'good' | 'warn' | 'bad' | 'none';

/**
 * Real-time health from the circuit breaker state (fed by live traffic and
 * the 60s auto-probe task), NOT from historical probe records or 24h
 * aggregates:
 * - all enabled endpoints available  → good
 * - some enabled endpoints down      → warn (degraded)
 * - no enabled endpoint available    → bad (unavailable)
 * - channel has no enabled endpoint  → none
 */
function channelHealth(ch: RoutingHealthChannel): Health {
  const enabled = ch.endpoints.filter((e) => e.enabled);
  if (enabled.length === 0) return 'none';
  const available = enabled.filter((e) => e.available).length;
  if (available === enabled.length) return 'good';
  if (available > 0) return 'warn';
  return 'bad';
}

function modelHealth(channels: RoutingHealthChannel[]): Health {
  const states = channels.map(channelHealth);
  if (states.length === 0) return 'none';
  if (states.some((s) => s === 'bad')) return 'bad';
  if (states.some((s) => s === 'warn')) return 'warn';
  return 'good';
}

interface ModelRow {
  id: string;
  name: string;
  pattern: string;
  published: boolean;
  contextLength: number | null;
  channelNames: string;
  requests: number; // 24h
  successRate: number; // 0..1, weighted by requests
  avgLatency: number;
  p95: number;
  cacheHitPct: number | null;
  health: Health;
  availableEps: number;
  enabledEps: number;
  brokenChannels: number;
  channels: RoutingHealthChannel[];
}

/**
 * Keep only the probe rows that represent the current health of a model:
 * per channel, prefer rows carrying an endpoint_url (real endpoint probes);
 * a synthetic failure row (endpoint_url = NULL, e.g. "Route not available")
 * is only considered when the channel has no endpoint probes at all. This
 * mirrors the backend's get_channel_health logic and prevents stale
 * NULL-url failures from flagging a model that now passes checks.
 */
function effectiveProbes(probeRows: ProbeResult[]): ProbeResult[] {
  const byChannel = new Map<string, ProbeResult[]>();
  for (const p of probeRows) {
    const arr = byChannel.get(p.channel_id) ?? [];
    arr.push(p);
    byChannel.set(p.channel_id, arr);
  }
  const out: ProbeResult[] = [];
  for (const rows of byChannel.values()) {
    const withUrl = rows.filter((r) => !!r.endpoint_url);
    out.push(...(withUrl.length > 0 ? withUrl : rows));
  }
  return out;
}

function buildRows(
  models: Model[] | undefined,
  rh: RoutingHealthResponse | undefined,
  ma: ModelActivity[] | undefined,
  channelName: Map<string, string>,
): ModelRow[] {
  if (!models) return [];
  const rhByName = new Map((rh?.models ?? []).map((m) => [m.name, m]));
  const maByName = new Map((ma ?? []).map((m) => [m.model, m]));

  const rows: ModelRow[] = [];
  for (const m of models) {
    const rhm: RoutingHealthModel | undefined = rhByName.get(m.name);
    const rhs = rhm?.channels ?? [];
    let totalReq = 0;
    let totalSuc = 0;
    let wLat = 0;
    let p95 = 0;
    let avail = 0;
    let enabled = 0;
    let broken = 0;
    for (const ch of rhs) {
      totalReq += ch.requests;
      totalSuc += ch.requests * ch.success_rate;
      wLat += ch.requests * ch.avg_latency_ms;
      if (ch.p95_latency_ms > p95) p95 = ch.p95_latency_ms;
      for (const ep of ch.endpoints) {
        if (ep.enabled) enabled++;
        if (ep.enabled && ep.available) avail++;
      }
      if (ch.requests > 0 && !ch.circuit_ok && ch.circuit_enabled) broken++;
    }
    const maRow = maByName.get(m.name);
    const inTokens = (maRow?.prompt_tokens ?? 0) + (maRow?.cache_hit_tokens ?? 0);
    const cacheHitPct = inTokens > 0 ? +(((maRow?.cache_hit_tokens ?? 0) / inTokens) * 100).toFixed(1) : null;
    const successRate = totalReq > 0 ? totalSuc / totalReq : 0;

    rows.push({
      id: m.id,
      name: m.name,
      pattern: m.model_pattern,
      published: !!m.published,
      contextLength: m.context_length ?? null,
      channelNames: m.channels
        .map((b) => channelName.get(b.channel_id) ?? b.channel_id)
        .join(' · '),
      requests: totalReq,
      successRate,
      avgLatency: totalReq > 0 ? wLat / totalReq : 0,
      p95,
      cacheHitPct,
      health: modelHealth(rhs),
      availableEps: avail,
      enabledEps: enabled,
      brokenChannels: broken,
      channels: rhs,
    });
  }
  return rows.sort((a, b) => b.requests - a.requests);
}

// ── live total (24h snapshot baseline + WS increments) ─────────────

export interface TimelineEntry {
  id: string;
  model: string;
  channel: string;
  endpointId?: number | null;
  /** Endpoint URL at request time — stable across endpoint re-creation,
   *  so the timeline can still match requests to current endpoints even
   *  when the endpoint row (and its DB id) has been re-created. */
  endpointUrl?: string;
  acceptedTs: number;
  completedTs?: number;
  latency?: number;
  success?: boolean;
}

const TIMELINE_CAP = 15;

function useLiveTotal() {
  const [totalCount, setTotalCount] = useState(0);
  const [connected, setConnected] = useState(false);
  const [reconnectIn, setReconnectIn] = useState(0);
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);

  useEffect(() => {
    fetchRoutingFlowSnapshot()
      .then((snap) => {
        const total = Object.entries(snap)
          .filter(([k]) => k.split('>').length === 1)
          .reduce((s, [, v]) => s + v, 0);
        setTotalCount(total);
      })
      .catch(() => {});
  }, []);

  // Seed the state timeline from ClickHouse so the grid isn't empty after
  // a page refresh — the endpoint status grid shows recent real traffic.
  // Live WS events then append on top (newer requests win the display).
  useEffect(() => {
    api<{ paths: { timestamp: string; model: string; channel_id: string; endpoint_id: number | null; endpoint_url: string | null; latency_ms: number; success: boolean }[] }>(
      '/health/recent-paths',
    )
      .then((r) => {
        const seeded: TimelineEntry[] = (r.paths ?? [])
          .filter((p) => p && typeof p.model === 'string' && typeof p.channel_id === 'string')
          .reverse() // CH returns newest-first; store oldest-first so display stays newest-first
          .map((p, i) => {
            const ts = Date.parse(p.timestamp);
            const tsNum = Number.isNaN(ts) ? Date.now() : ts;
            return {
              id: `seed-${i}-${p.endpoint_id ?? 'n'}`,
              model: p.model,
              channel: p.channel_id,
              endpointId: p.endpoint_id ?? null,
              endpointUrl: p.endpoint_url ?? undefined,
              acceptedTs: tsNum - p.latency_ms,
              completedTs: tsNum,
              latency: p.latency_ms,
              success: p.success,
            };
          });
        if (seeded.length === 0) return;
        setTimeline((prev) => {
          const merged = [...seeded, ...prev];
          return merged.length > TIMELINE_CAP ? merged.slice(merged.length - TIMELINE_CAP) : merged;
        });
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    function connect() {
      const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${proto}://${window.location.host}/api/health/ws`);
      ws.onopen = () => {
        setConnected(true);
        setReconnectIn(0);
        if (timer) {
          clearInterval(timer);
          timer = null;
        }
      };
      ws.onmessage = (e) => {
        let ev: {
          type?: string;
          model?: string;
          channel_id?: string;
          request_id?: string;
          endpoint_id?: number | null;
          latency_ms?: number;
          success?: boolean;
          timestamp?: string;
        };
        try {
          ev = JSON.parse(e.data) as typeof ev;
        } catch {
          return;
        }
        if (typeof ev.model !== 'string' || typeof ev.channel_id !== 'string') return;
        const isDecided = ev.type === 'route_decided' || (ev.type == null && ev.latency_ms == null);
        if (isDecided) setTotalCount((c) => c + 1);

        // State timeline: RouteDecided (no latency) opens a request row;
        // RequestCompleted (latency_ms) closes it.
        if (typeof ev.request_id === 'string') {
          const ts = Date.parse(ev.timestamp ?? '');
          const tsNum = Number.isNaN(ts) ? Date.now() : ts;
          setTimeline((prev) => {
            let next = prev.slice();
            if (ev.latency_ms != null) {
              const idx = next.findIndex((e) => e.id === ev.request_id);
              const entry = {
                id: ev.request_id!,
                model: ev.model!,
                channel: ev.channel_id!,
                endpointId: ev.endpoint_id ?? null,
                acceptedTs: tsNum - ev.latency_ms,
                completedTs: tsNum,
                latency: ev.latency_ms,
                success: ev.success,
              };
              if (idx >= 0) next[idx] = entry;
              else next.push(entry);
            } else if (!next.some((e) => e.id === ev.request_id)) {
              next.push({
                id: ev.request_id!,
                model: ev.model!,
                channel: ev.channel_id!,
                endpointId: ev.endpoint_id ?? null,
                acceptedTs: tsNum,
              });
            }
            if (next.length > TIMELINE_CAP) next = next.slice(next.length - TIMELINE_CAP);
            return next;
          });
        }
      };
      ws.onclose = () => {
        setConnected(false);
        if (!closed) {
          let c = 3;
          setReconnectIn(c);
          timer = setInterval(() => {
            c--;
            if (c <= 0) {
              if (timer) {
                clearInterval(timer);
                timer = null;
              }
              setTimeout(connect, 500);
            } else {
              setReconnectIn(c);
            }
          }, 1000);
        }
      };
      ws.onerror = () => {
        try {
          ws?.close();
        } catch {
          // ignore
        }
      };
    }
    connect();
    return () => {
      closed = true;
      if (timer) clearInterval(timer);
      try {
        ws?.close();
      } catch {
        // ignore
      }
    };
  }, []);

  return { totalCount, connected, reconnectIn, timeline };
}

// ── top status strip ───────────────────────────────────────────────

function StatusCell({ label, value, foot, tone }: {
  label: string;
  value: string;
  foot?: string;
  tone?: 'good' | 'warn' | 'bad';
}) {
  return (
    <div className="border-t lg:border-t-0 lg:border-l border-border/60 px-5 py-4 min-w-0">
      <div className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div
        className={cn(
          'mt-1.5 text-2xl font-bold tracking-tight tabular-nums',
          tone === 'good' && 'text-emerald-600 dark:text-emerald-400',
          tone === 'warn' && 'text-amber-600 dark:text-amber-400',
          tone === 'bad' && 'text-destructive',
        )}
      >
        {value}
      </div>
      {foot && <div className="mt-0.5 text-[11px] text-muted-foreground">{foot}</div>}
    </div>
  );
}

function StatusStrip({
  leadTitle, leadCopy, availability, currentRequests, p95, totalTokens, cachePct, connected,
}: {
  leadTitle: string;
  leadCopy: string;
  availability: number;
  currentRequests: number;
  p95: number;
  totalTokens: number;
  cachePct: number | null;
  connected: boolean;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="grid grid-cols-2 lg:grid-cols-[1.25fr_repeat(4,minmax(0,1fr))]">
        <div className="col-span-2 lg:col-span-1 flex items-center gap-4 px-5 py-4 min-w-0">
          <div className="relative size-11 shrink-0 rounded-full bg-gradient-to-br from-emerald-300 via-emerald-500 to-emerald-900 grid place-items-center shadow-[0_0_0_8px_rgba(16,185,129,0.07),0_0_28px_rgba(16,185,129,0.18)]">
            <span className="absolute inset-0 rounded-full border border-emerald-400/40 animate-ping opacity-60" />
          </div>
          <div className="min-w-0">
            <div className="text-base font-bold truncate">{leadTitle}</div>
            <div className="text-xs text-muted-foreground truncate">{leadCopy}</div>
          </div>
        </div>
        <StatusCell
          label={t('monitor.globalAvailability')}
          value={`${availability.toFixed(2)}%`}
          foot={t('monitor.sloFoot')}
          tone={availability >= 99 ? 'good' : 'warn'}
        />
        <StatusCell
          label={t('monitor.currentRequests')}
          value={fmtCount(currentRequests)}
          foot={connected ? t('monitor.live') : t('monitor.liveOffline')}
        />
        <StatusCell
          label={t('monitor.p95Latency')}
          value={fmtLat(p95)}
          tone={p95 > 5000 ? 'warn' : undefined}
        />
        <StatusCell
          label={t('monitor.todayTokens')}
          value={fmtTokens(totalTokens)}
          foot={cachePct !== null ? t('monitor.cacheHit', { pct: cachePct }) : undefined}
        />
      </div>
    </section>
  );
}

// ── left: model catalog ────────────────────────────────────────────

const HEALTH_DOT: Record<Health, string> = {
  good: 'bg-emerald-500 shadow-[0_0_0_4px_rgba(16,185,129,0.12)]',
  warn: 'bg-amber-500 shadow-[0_0_0_4px_rgba(245,158,11,0.12)]',
  bad: 'bg-red-500 shadow-[0_0_0_4px_rgba(239,68,68,0.12)]',
  none: 'bg-muted-foreground/40',
};

function ModelCatalog({
  rows, search, onSearch, selectedName, onSelect,
}: {
  rows: ModelRow[];
  search: string;
  onSearch: (v: string) => void;
  selectedName: string | null;
  onSelect: (name: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 min-h-[52px] border-b border-border/60">
        <div className="text-sm font-bold">{t('monitor.modelCatalog')}</div>
        <div className="text-[11px] text-muted-foreground">
          {t('monitor.modelsCount', { n: rows.length })}
        </div>
      </div>
      <div className="p-2.5 border-b border-border/60">
        <div className="flex items-center gap-2 h-9 rounded-lg border bg-muted/40 px-2.5 text-muted-foreground">
          <Search className="size-3.5 shrink-0" />
          <input
            value={search}
            onChange={(e) => onSearch(e.target.value)}
            placeholder={t('monitor.searchPlaceholder')}
            className="w-full bg-transparent text-sm outline-none text-foreground placeholder:text-muted-foreground"
          />
        </div>
      </div>
      <div className="max-h-[620px] overflow-y-auto p-1.5">
        {rows.length === 0 ? (
          <div className="py-10 text-center text-xs text-muted-foreground">
            {t('monitor.emptyModels')}
          </div>
        ) : (
          rows.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => onSelect(r.name)}
              className={cn(
                'w-full text-left grid grid-cols-[8px_1fr_auto] gap-2 items-start rounded-lg px-2.5 py-2.5 mb-0.5 border border-transparent transition-colors cursor-pointer',
                r.name === selectedName
                  ? 'bg-brand/10 border-brand/20'
                  : 'hover:bg-muted/50',
              )}
            >
              <span className={cn('size-2 rounded-full mt-1.5', HEALTH_DOT[r.health])} />
              <span className="min-w-0">
                <span className="block text-xs font-semibold truncate">{r.name}</span>
                <span className="block text-[10px] text-muted-foreground mt-0.5 truncate">
                  {r.channelNames || '—'}
                </span>
              </span>
              <span className="text-[11px] text-muted-foreground tabular-nums pt-0.5">
                {fmtCount(r.requests)}
              </span>
            </button>
          ))
        )}
      </div>
    </section>
  );
}

// ── center: performance chart ──────────────────────────────────────

type MetricKey = 'requests' | 'tokens' | 'latency' | 'error';

function PerformanceChart({ data }: {
  data: { time: string; count: number; tokens: number; latency: number; error: number }[];
}) {
  const { t } = useTranslation();
  const [metric, setMetric] = useState<MetricKey>('requests');

  const METRICS: { key: MetricKey; label: string; dataKey: string; unit: string }[] = [
    { key: 'requests', label: t('monitor.metricRequests'), dataKey: 'count', unit: '' },
    { key: 'tokens', label: t('monitor.metricTokens'), dataKey: 'tokens', unit: '' },
    { key: 'latency', label: t('monitor.metricLatency'), dataKey: 'latency', unit: 'ms' },
    { key: 'error', label: t('monitor.metricErrors'), dataKey: 'error', unit: '%' },
  ];
  const active = METRICS.find((m) => m.key === metric)!;

  const main = useMemo(() => {
    if (data.length === 0) return null;
    switch (metric) {
      case 'requests':
        return { v: data.reduce((s, d) => s + d.count, 0), unit: t('monitor.req24h') };
      case 'tokens':
        return { v: data.reduce((s, d) => s + d.tokens, 0), unit: '' };
      case 'latency': {
        const last = [...data].reverse().find((d) => d.latency > 0);
        return last ? { v: last.latency, unit: 'ms' } : null;
      }
      case 'error': {
        const last = [...data].reverse().find((d) => d.count > 0);
        return last ? { v: last.error, unit: '%' } : null;
      }
    }
  }, [data, metric, t]);

  const yFmt = (v: number) => {
    if (metric === 'latency') return fmtLat(v);
    if (metric === 'error') return `${v}%`;
    if (metric === 'tokens') return fmtTokens(v);
    return fmtCount(v);
  };

  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-2 px-4 min-h-[52px] border-b border-border/60">
        <div>
          <div className="text-sm font-bold">{t('monitor.realtimePerf')}</div>
          <div className="text-[11px] text-muted-foreground">{t('monitor.recent24h')}</div>
        </div>
        <div className="flex gap-0.5">
          {METRICS.map((m) => (
            <button
              key={m.key}
              type="button"
              onClick={() => setMetric(m.key)}
              className={cn(
                'px-2.5 py-1 rounded-md text-xs cursor-pointer transition-colors',
                metric === m.key
                  ? 'bg-muted text-foreground font-semibold'
                  : 'text-muted-foreground hover:text-foreground',
              )}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>
      <div className="px-4 pt-3 pb-2">
        <div className="flex items-baseline gap-2">
          {main ? (
            <>
              <span className="text-[26px] font-extrabold tracking-tight tabular-nums">
                {metric === 'latency' ? fmtLat(main.v) : metric === 'error' ? `${main.v}%` : fmtCount(main.v)}
              </span>
              <span className="text-[11px] text-muted-foreground">{main.unit}</span>
            </>
          ) : (
            <span className="text-sm text-muted-foreground">{t('common.loading')}</span>
          )}
        </div>
      </div>
      <div className="px-3 pb-3">
        {data.length > 0 ? (
          <ResponsiveContainer width="100%" height={210}>
            <AreaChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
              <defs>
                <linearGradient id="monArea" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.25} />
                  <stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
              <XAxis
                dataKey="time"
                tickLine={false}
                axisLine={false}
                tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }}
                minTickGap={48}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                tick={{ fill: 'var(--muted-foreground)', fontSize: 10 }}
                tickFormatter={yFmt}
                width={52}
              />
              <Tooltip
                formatter={(value, name) => [yFmt(Number(value ?? 0)), String(name)]}
                labelFormatter={(label) => String(label)}
              />
              <Area
                type="monotone"
                dataKey={active.dataKey}
                stroke="var(--chart-1)"
                strokeWidth={2}
                fill="url(#monArea)"
              />
            </AreaChart>
          </ResponsiveContainer>
        ) : (
          <div className="h-[210px] grid place-items-center text-xs text-muted-foreground">
            {t('monitor.noChartData')}
          </div>
        )}
      </div>
    </section>
  );
}

// ── center: request flow ───────────────────────────────────────────

function FlowLink({ delay }: { delay?: string }) {
  return (
    <div className="mon-link relative h-px bg-gradient-to-r from-border via-brand to-border mx-1 flex-1 min-w-[20px]">
      <i style={delay ? { animationDelay: delay } : undefined} />
    </div>
  );
}

// ── center: request state timeline ─────────────────────────────────

function StateTimeline({ timeline }: { timeline: TimelineEntry[] }) {
  const { t } = useTranslation();
  if (timeline.length === 0) {
    return (
      <div className="px-4 pb-4">
        <div className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground mb-2">
          {t('monitor.stateTimeline')}
        </div>
        <div className="py-3 text-center text-xs text-muted-foreground">{t('monitor.noLiveData')}</div>
      </div>
    );
  }
  const now = Date.now();
  const entries = [...timeline].reverse(); // newest first
  const minTs = Math.min(...entries.map((e) => e.acceptedTs));
  const maxTs = Math.max(...entries.map((e) => e.completedTs ?? now));
  const span = Math.max(maxTs - minTs, 1);
  const pct = (v: number) => `${Math.min(100, Math.max(0, (v / span) * 100))}%`;
  return (
    <div className="px-4 pb-4">
      <div className="flex items-center justify-between text-[10px] font-medium uppercase tracking-wider text-muted-foreground mb-2">
        <span>{t('monitor.stateTimeline')}</span>
        <span className="tabular-nums">{entries.length}</span>
      </div>
      <div className="space-y-1.5">
        {entries.map((e) => {
          const start = pct(e.acceptedTs - minTs);
          const width = e.completedTs ? pct(e.completedTs - e.acceptedTs) : undefined;
          const label = e.model || e.id.slice(0, 8);
          return (
            <div key={e.id} className="flex items-center gap-2 text-[10px]">
              <span
                className="w-24 truncate text-muted-foreground shrink-0"
                title={`${e.model} · ${e.channel} · ${e.id}`}
              >
                {label.length > 12 ? `${label.slice(0, 11)}…` : label}
              </span>
              <div className="relative flex-1 h-4 rounded bg-muted/30 overflow-hidden">
                <span className="absolute top-0 bottom-0 w-px bg-border/80" style={{ left: start }} />
                {e.completedTs ? (
                  <span
                    className={cn(
                      'absolute top-1 bottom-1 rounded-sm',
                      e.success === false ? 'bg-red-500/60' : 'bg-emerald-500/60',
                    )}
                    style={{ left: start, width }}
                  />
                ) : (
                  <span
                    className="absolute top-1 bottom-1 w-1.5 rounded-full bg-blue-500 animate-pulse"
                    style={{ left: start }}
                  />
                )}
              </div>
              <span className="w-16 text-right tabular-nums text-muted-foreground shrink-0">
                {e.completedTs ? `${e.latency ?? 0}ms` : t('monitor.inFlight')}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RequestFlow({
  ingress, directPass, upstream, rl, af, cachePct, p95, timeout, timeline,
}: {
  ingress: number;
  directPass: number;
  upstream: number;
  rl: number;
  af: number;
  cachePct: number | null;
  p95: number;
  timeout: number;
  timeline: TimelineEntry[];
}) {
  const { t } = useTranslation();
  const node = (kicker: string, value: string, valueUnit: string, detail: string, gateway = false) => (
    <div className={cn('relative min-h-[84px] rounded-lg border p-3 bg-muted/20 overflow-hidden', gateway && 'border-brand/40')}>
      <span className={cn('absolute inset-y-0 left-0 w-0.5', gateway ? 'bg-brand shadow-[0_0_14px_var(--brand)]' : 'bg-border')} />
      <div className="text-[9px] uppercase tracking-widest text-muted-foreground">{kicker}</div>
      <div className="mt-1.5 text-lg font-bold tabular-nums leading-none">
        {value}
        {valueUnit && <span className="ml-1 text-[10px] font-medium text-muted-foreground">{valueUnit}</span>}
      </div>
      <div className="mt-1.5 text-[10px] text-muted-foreground leading-snug">{detail}</div>
    </div>
  );

  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 min-h-[52px] border-b border-border/60">
        <div className="text-sm font-bold">{t('monitor.reqFlow')}</div>
        <div className="text-[11px] text-muted-foreground">{t('monitor.recent24h')}</div>
      </div>
      <div className="p-4 flex items-center">
        <div className="flex-1 min-w-0">
          {node(
            t('monitor.ingress'),
            fmtCount(ingress),
            'req',
            t('monitor.ingressDetail', { n: fmtCount(ingress) }),
          )}
        </div>
        <FlowLink />
        <div className="flex-1 min-w-0">
          {node(
            t('monitor.gateway'),
            `${directPass.toFixed(1)}%`,
            t('monitor.directPass'),
            t('monitor.gatewayDetail', { rl: fmtCount(rl), af: fmtCount(af), cache: cachePct ?? 0 }),
            true,
          )}
        </div>
        <FlowLink delay="-0.9s" />
        <div className="flex-1 min-w-0">
          {node(
            t('monitor.upstream'),
            fmtCount(upstream),
            'req',
            t('monitor.upstreamDetail', { p95: fmtLat(p95), to: fmtCount(timeout) }),
          )}
        </div>
      </div>

      {/* State timeline: recent requests accepted → completed */}
      <div className="border-t border-border/60">
        <StateTimeline timeline={timeline} />
      </div>
    </section>
  );
}

// ── center: channel endpoint status ────────────────────────────────

const EP_BADGE: Record<'good' | 'bad' | 'none', string> = {
  good: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  bad: 'bg-red-500/10 text-red-700 dark:text-red-400',
  none: 'bg-muted text-muted-foreground',
};

// ── endpoint state timeline grid ────────────────────────────────────
// One row per endpoint with 18 time-bucket cells. Cell width = probe
// interval from gateway settings. Hover a cell for timestamp + status.
// The probe-poll window is a fixed 30 minutes (stable, not tied to
// interval) so the grid doesn't jitter when the settings load later.

const EP_TIMELINE_COLS = 18;
const EP_TIMELINE_MINUTES = 30;

/** Read the configured probe interval from settings. */
function useProbeInterval(): number {
  const [intervalSecs, setIntervalSecs] = useState(60);
  useEffect(() => {
    api<{ interval_secs: number }>('/settings/probe-interval')
      .then((r) => setIntervalSecs(r.interval_secs))
      .catch(() => {});
  }, []);
  return intervalSecs;
}

/** Poll recent raw probe results (fixed window, not tied to interval). */
function useRecentProbes() {
  const [probes, setProbes] = useState<ProbeResult[]>([]);
  useEffect(() => {
    let active = true;
    const load = () => {
      api<ProbeResult[]>(`/probe-results/recent?minutes=${EP_TIMELINE_MINUTES}`)
        .then((r) => {
          if (active) setProbes(r ?? []);
        })
        .catch(() => {});
    };
    load();
    const t = setInterval(load, 5000);
    return () => {
      active = false;
      clearInterval(t);
    };
  }, []);
  return probes;
}

function EndpointTimeline({
  row, endpointUrl, channelName,
}: {
  row: ModelRow | null;
  endpointUrl: Map<string, Map<number, string>>;
  channelName: Map<string, string>;
}) {
  const { t } = useTranslation();
  const intervalSecs = useProbeInterval();
  const probes = useRecentProbes();

  const endpoints = useMemo(() => {
    if (!row) return [];
    const list: { channelId: string; channelName: string; url: string }[] = [];
    for (const ch of row.channels) {
      const chUrlMap = endpointUrl.get(ch.channel_id);
      for (const ep of ch.endpoints) {
        const url =
          (ep.endpoint_id != null && chUrlMap?.get(ep.endpoint_id)) || '';
        list.push({
          channelId: ch.channel_id,
          channelName: channelName.get(ch.channel_id) || ch.channel_name || ch.channel_id,
          url,
        });
      }
    }
    return list;
  }, [row, endpointUrl, channelName]);

  // Cell width = probe interval from settings. The timeline anchor is
  // aligned to the bucketMs boundary so grid cells are stable across
  // re-renders — otherwise changing now shifts all cell boundaries and
  // makes probe data hop between cells on every 5s poll.
  const bucketMs = Math.max(1000, intervalSecs * 1000);
  const nowAligned = Math.floor(Date.now() / bucketMs) * bucketMs;
  const windowStart = nowAligned - EP_TIMELINE_COLS * bucketMs;

  const hitsForCell = useCallback(
    (ep: { channelId: string; url: string }, i: number): { n: number; ok: number; fail: number; times: string[] } => {
      const start = windowStart + i * bucketMs;
      const end = start + bucketMs;
      let ok = 0, fail = 0;
      const times: string[] = [];
      for (const p of probes) {
        if (p.channel_id !== ep.channelId) continue;
        if (ep.url && p.endpoint_url !== ep.url) continue;
        const ts = Date.parse(p.probed_at);
        if (Number.isNaN(ts) || ts < start || ts >= end) continue;
        const d = new Date(ts);
        times.push(d.toLocaleTimeString());
        if (p.success) ok++; else fail++;
      }
      return { n: ok + fail, ok, fail, times };
    },
    [probes, windowStart, bucketMs],
  );

  const cellClass = (ok: number, fail: number) => {
    if (fail > 0) return 'bg-red-500';
    if (ok > 0) return 'bg-emerald-500';
    return 'bg-muted-foreground/20';
  };

  if (!row || endpoints.length === 0) return null;

  return (
    <div className="border-t border-border/60 px-4 py-3">
      <style>{`
        .ep-cell { position: relative; }
        .ep-cell:hover { outline: 2px solid #fff; }
        .ep-cell:hover::after {
          content: attr(data-tip);
          position: absolute;
          bottom: 28px;
          left: 50%;
          transform: translateX(-50%);
          background: hsl(var(--popover));
          border: 1px solid hsl(var(--border));
          padding: 6px 8px;
          border-radius: 6px;
          font-size: 11px;
          white-space: pre;
          line-height: 1.5;
          z-index: 30;
          pointer-events: none;
        }
      `}</style>
      <div className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground mb-2">
        {t('monitor.endpointTimeline')}
      </div>
      <div className="overflow-x-auto relative">
        {endpoints.map((ep, ri) => (
          <div key={`${ep.channelId}-${ri}`} className="flex items-start gap-4 mb-5">
            <div className="w-44 shrink-0 min-w-0 pt-0.5">
              <div className="text-xs font-semibold truncate">{ep.channelName}</div>
              <div className="text-[11px] text-muted-foreground truncate">{ep.url || '—'}</div>
            </div>
            <div className="flex gap-1.5 flex-wrap">
              {Array.from({ length: EP_TIMELINE_COLS }, (_, i) => {
                const h = hitsForCell(ep, i);
                const cellTs = windowStart + i * bucketMs;
                const cls = cellClass(h.ok, h.fail);
                const ago = Math.floor((nowAligned - cellTs) / 60000);
                const tooltipLines = [
                  `${ago}m ago · ${h.fail > 0 ? 'FAIL' : h.ok > 0 ? 'OK' : 'NO DATA'}`,
                  h.n > 0 ? `probes ${h.n} · ${h.ok} ok / ${h.fail} fail` : '',
                ]
                  .filter(Boolean)
                  .join('\n');
                return (
                  <span
                    key={i}
                    className={`ep-cell inline-block w-[22px] h-[22px] rounded-sm ${cls}`}
                    data-tip={tooltipLines}
                  />
                );
              })}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function ChannelEndpointStatus({
  row, channelName, endpointUrl,
}: {
  row: ModelRow | null;
  channelName: Map<string, string>;
  endpointUrl: Map<string, Map<number, string>>;
}) {
  const { t } = useTranslation();

  if (!row) {
    return (
      <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
        <div className="px-4 min-h-[52px] flex items-center border-b border-border/60">
          <div className="text-sm font-bold">{t('monitor.channelEndpoints')}</div>
        </div>
        <div className="p-6 text-center text-xs text-muted-foreground">{t('monitor.noSelection')}</div>
      </section>
    );
  }

  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 min-h-[52px] border-b border-border/60">
        <div className="text-sm font-bold">{t('monitor.channelEndpoints')}</div>
        <div className="text-[11px] text-muted-foreground">{row.name}</div>
      </div>
      <div className="divide-y divide-border/40">
        {row.channels.length === 0 ? (
          <div className="p-6 text-center text-xs text-muted-foreground">
            {t('monitor.noChannelData')}
          </div>
        ) : (
          row.channels.map((ch) => {
            const enabled = ch.endpoints.filter((e) => e.enabled);
            const available = enabled.filter((e) => e.available);
            const state: 'good' | 'bad' | 'none' =
              enabled.length === 0 ? 'none' : available.length === enabled.length ? 'good' : 'bad';
            const label =
              state === 'good'
                ? `${available.length}/${enabled.length} ${t('monitor.epAvailable')}`
                : state === 'none'
                  ? t('monitor.epDisabled')
                  : `${available.length}/${enabled.length} ${t('monitor.epUnavailable')}`;
            return (
              <div key={ch.channel_id} className="px-4 py-3">
                <div className="flex items-center justify-between gap-3 flex-wrap">
                  <div className="min-w-0">
                    <span className="text-xs font-semibold">
                      {ch.channel_name || channelName.get(ch.channel_id) || ch.channel_id}
                    </span>
                    <span className="text-[10px] text-muted-foreground font-mono ml-2">{ch.channel_id}</span>
                  </div>
                  <div className="flex items-center gap-3 text-[10px] text-muted-foreground tabular-nums">
                    <span>{fmtCount(ch.requests)} req</span>
                    <span>{ch.requests > 0 ? `${(ch.success_rate * 100).toFixed(1)}%` : '—'}</span>
                    <span>P95 {fmtLat(ch.p95_latency_ms)}</span>
                    <span className={cn('inline-flex items-center rounded-full px-2 py-0.5 font-medium', EP_BADGE[state])}>
                      {label}
                    </span>
                  </div>
                </div>
                <div className="mt-2 space-y-1">
                  {ch.endpoints.map((ep, i) => {
                    const url =
                      (ep.endpoint_id != null && endpointUrl.get(ch.channel_id)?.get(ep.endpoint_id)) ??
                      `#${i + 1}`;
                    const epState = !ep.enabled ? 'none' : ep.available ? 'good' : 'bad';
                    const epLabel = !ep.enabled
                      ? t('monitor.epDisabled')
                      : ep.available
                        ? t('monitor.epAvailable')
                        : t('monitor.epUnavailable');
                    return (
                      <div key={ep.endpoint_id ?? i} className="flex items-center justify-between gap-3 text-xs">
                        <span className="font-mono text-muted-foreground truncate">{url}</span>
                        <span className={cn('inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium shrink-0', EP_BADGE[epState])}>
                          {epLabel}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            );
          })
        )}
      </div>
      <EndpointTimeline row={row} endpointUrl={endpointUrl} channelName={channelName} />
    </section>
  );
}

// ── center: model compare table ────────────────────────────────────

const HEALTH_BADGE: Record<Health, string> = {
  good: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  warn: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  bad: 'bg-red-500/10 text-red-700 dark:text-red-400',
  none: 'bg-muted text-muted-foreground',
};

function ModelCompareTable({
  rows, selectedName, onSelect,
}: {
  rows: ModelRow[];
  selectedName: string | null;
  onSelect: (name: string) => void;
}) {
  const { t } = useTranslation();
  const statusLabel = (h: Health) =>
    h === 'good'
      ? t('monitor.statusHealthy')
      : h === 'warn'
        ? t('monitor.statusDegraded')
        : h === 'bad'
          ? t('monitor.statusMaintenance')
          : t('monitor.statusUntested');

  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 min-h-[52px] border-b border-border/60">
        <div>
          <div className="text-sm font-bold">{t('monitor.modelCompare')}</div>
          <div className="text-[11px] text-muted-foreground">{t('monitor.compareSubtitle')}</div>
        </div>
        <div className="text-[11px] text-muted-foreground">{t('monitor.byRequests')}</div>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full text-xs min-w-[760px]">
          <thead>
            <tr className="border-b border-border/60 text-muted-foreground">
              <th className="text-left font-semibold uppercase tracking-wider px-4 py-2.5">{t('monitor.colModel')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colStatus')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colRps')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colP95')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colAvgLat')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colError')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-3 py-2.5">{t('monitor.colCache')}</th>
              <th className="text-right font-semibold uppercase tracking-wider px-4 py-2.5">{t('monitor.colCost')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr
                key={r.id}
                onClick={() => onSelect(r.name)}
                className={cn(
                  'border-b border-border/40 last:border-0 cursor-pointer transition-colors',
                  r.name === selectedName ? 'bg-brand/5' : 'hover:bg-muted/40',
                )}
              >
                <td className="px-4 py-2.5">
                  <span className="flex items-center gap-2 font-semibold">
                    <span className={cn('size-1.5 rounded-full', HEALTH_DOT[r.health])} />
                    {r.name}
                  </span>
                </td>
                <td className="px-3 py-2.5 text-right">
                  <span className={cn('inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium', HEALTH_BADGE[r.health])}>
                    {statusLabel(r.health)}
                  </span>
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums">{fmtCount(r.requests)}</td>
                <td className="px-3 py-2.5 text-right tabular-nums">{r.p95 > 0 ? fmtLat(r.p95) : '—'}</td>
                <td className="px-3 py-2.5 text-right tabular-nums">{r.avgLatency > 0 ? fmtLat(r.avgLatency) : '—'}</td>
                <td className="px-3 py-2.5 text-right tabular-nums">
                  {r.requests > 0 ? `${((1 - r.successRate) * 100).toFixed(2)}%` : '—'}
                </td>
                <td className="px-3 py-2.5 text-right tabular-nums">
                  {r.cacheHitPct !== null ? `${r.cacheHitPct}%` : '—'}
                </td>
                <td className="px-4 py-2.5 text-right tabular-nums text-muted-foreground">—</td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={8} className="py-10 text-center text-muted-foreground">
                  {t('monitor.emptyModels')}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}

// ── right: inspector ───────────────────────────────────────────────

function AvailabilityRing({ pct, tone }: { pct: number; tone: Health }) {
  const { t } = useTranslation();
  const color = tone === 'bad' ? 'hsl(var(--destructive))' : tone === 'warn' ? '#f59e0b' : 'hsl(var(--primary))';
  return (
    <div
      className="relative size-[92px] shrink-0 rounded-full grid place-items-center"
      style={{ background: `conic-gradient(${color} ${pct * 3.6}deg, hsl(var(--border)) 0deg)` }}
    >
      <div className="absolute inset-2 rounded-full bg-card border border-border" />
      <div className="relative z-10 text-center">
        <b className="block text-lg font-bold tabular-nums">{pct.toFixed(2)}%</b>
        <span className="text-[9px] text-muted-foreground">{t('monitor.availability')}</span>
      </div>
    </div>
  );
}

function ModelInspector({ row }: { row: ModelRow | null }) {
  const { t } = useTranslation();
  if (!row) {
    return (
      <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
        <div className="px-4 min-h-[52px] flex items-center justify-between border-b border-border/60">
          <div className="text-sm font-bold">{t('monitor.inspector')}</div>
        </div>
        <div className="p-6 text-center text-xs text-muted-foreground">{t('monitor.noSelection')}</div>
      </section>
    );
  }

  const availability = row.requests > 0 ? row.successRate * 100 : 0;
  const tone: Health = row.health;
  const kv = (label: string, value: string) => (
    <div className="rounded-lg border border-border/60 bg-muted/20 p-2.5 min-w-0">
      <div className="text-[9px] font-medium uppercase tracking-wider text-muted-foreground">{label}</div>
      <b className="block mt-1 text-sm tabular-nums truncate">{value}</b>
    </div>
  );
  const progress = (label: string, pct: number, color: 'brand' | 'amber' | 'blue') => (
    <div>
      <div className="flex justify-between text-[10px] text-muted-foreground mb-1">
        <span>{label}</span>
        <span className="tabular-nums">{pct.toFixed(1)}%</span>
      </div>
      <div className="h-1.5 rounded-full bg-muted overflow-hidden">
        <i
          className={cn(
            'block h-full rounded-full transition-all',
            color === 'brand' && 'bg-brand',
            color === 'amber' && 'bg-amber-500',
            color === 'blue' && 'bg-blue-500',
          )}
          style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
        />
      </div>
    </div>
  );

  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="px-4 min-h-[52px] flex items-center justify-between border-b border-border/60">
        <div className="text-sm font-bold">{t('monitor.inspector')}</div>
        <div className="text-[11px] text-muted-foreground">{t('monitor.live')}</div>
      </div>
      <div className="p-4 space-y-4">
        <div className="min-w-0">
          <div className="text-[17px] font-extrabold tracking-tight truncate">{row.name}</div>
          <div className="text-[10px] text-muted-foreground mt-0.5 font-mono truncate">{row.id}</div>
        </div>

        <div className="flex items-center gap-4">
          <AvailabilityRing pct={availability} tone={tone} />
          <div className="grid gap-1.5 text-[10px] min-w-0">
            <div className="flex justify-between gap-6">
              <span className="text-muted-foreground">{t('monitor.published')}</span>
              <span className="text-foreground">{row.published ? t('monitor.yes') : t('monitor.no')}</span>
            </div>
            <div className="flex justify-between gap-6">
              <span className="text-muted-foreground">{t('monitor.channels')}</span>
              <span className="text-foreground tabular-nums">{row.channels.length || 0}</span>
            </div>
            <div className="flex justify-between gap-6">
              <span className="text-muted-foreground">{t('monitor.contextLength')}</span>
              <span className="text-foreground tabular-nums">
                {row.contextLength ? fmtTokens(row.contextLength) : '—'}
              </span>
            </div>
          </div>
        </div>

        <div className="h-px bg-border/60" />

        <div className="grid grid-cols-2 gap-2">
          {kv(
            t('monitor.activeInstances'),
            row.enabledEps > 0 ? `${row.availableEps} / ${row.enabledEps}` : '—',
          )}
          {kv(t('monitor.requests24h'), fmtCount(row.requests))}
          {kv(t('monitor.cacheHitRate'), row.cacheHitPct !== null ? `${row.cacheHitPct}%` : '—')}
          {kv(t('monitor.avgLatency'), row.avgLatency > 0 ? fmtLat(row.avgLatency) : '—')}
        </div>

        <div className="grid gap-3">
          {progress(t('monitor.cacheHitRate'), row.cacheHitPct ?? 0, 'blue')}
          {progress(t('monitor.successRate'), row.requests > 0 ? row.successRate * 100 : 0, 'brand')}
        </div>
      </div>
    </section>
  );
}

// ── right: incidents ───────────────────────────────────────────────

function IncidentList({ incidents }: {
  incidents: { key: string; kind: 'red' | 'amber' | 'blue'; title: string; meta: string }[];
}) {
  const { t } = useTranslation();
  return (
    <section className="rounded-xl border bg-card/80 shadow-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 min-h-[52px] border-b border-border/60">
        <div className="text-sm font-bold">{t('monitor.incidents')}</div>
        <div className="text-[11px] text-muted-foreground">
          {t('monitor.incidentsCount', { n: incidents.length })}
        </div>
      </div>
      <div className="px-4 pb-2">
        {incidents.length === 0 ? (
          <div className="py-8 text-center text-xs text-muted-foreground">{t('monitor.noIncidents')}</div>
        ) : (
          incidents.map((inc) => (
            <div key={inc.key} className="grid grid-cols-[8px_1fr] gap-2.5 py-3 border-b border-border/40 last:border-0">
              <span
                className={cn(
                  'size-1.5 rounded-full mt-1.5',
                  inc.kind === 'red' && 'bg-red-500 shadow-[0_0_0_4px_rgba(239,68,68,0.1)]',
                  inc.kind === 'amber' && 'bg-amber-500 shadow-[0_0_0_4px_rgba(245,158,11,0.1)]',
                  inc.kind === 'blue' && 'bg-blue-500 shadow-[0_0_0_4px_rgba(59,130,246,0.1)]',
                )}
              />
              <div className="min-w-0">
                <div className="text-[11px] leading-snug">{inc.title}</div>
                <div className="text-[9px] text-muted-foreground mt-1">{inc.meta}</div>
              </div>
            </div>
          ))
        )}
      </div>
    </section>
  );
}

// ── page ───────────────────────────────────────────────────────────

export default function FlowTowerContent() {
  const { t } = useTranslation();
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [search, setSearch] = useState('');

  const { data: models } = usePublicModels();  const { data: channels } = useChannels();
  const { data: rh } = useRoutingHealth();
  const { data: agg } = useDashboardAggregations();
  const { data: funnel } = useUsageFunnel(1);
  const { data: ua } = useUsageAggregate(1);
  const { data: ma } = useModelActivity(1);
  const { data: probes } = useProbeResults();
  const { totalCount, connected, timeline } = useLiveTotal();

  const channelName = useMemo(
    () => new Map((channels ?? []).map((c) => [c.id, c.name || c.id])),
    [channels],
  );

  // channelId → endpointId → url (for the endpoint status panel)
  const endpointUrl = useMemo(() => {
    const map = new Map<string, Map<number, string>>();
    for (const c of channels ?? []) {
      const byId = new Map<number, string>();
      for (const ep of c.endpoints) {
        if (ep.id != null) byId.set(ep.id, ep.url);
      }
      map.set(c.id, byId);
    }
    return map;
  }, [channels]);

  const probeByName = useMemo(() => {
    const map = new Map<string, ProbeResult[]>();
    for (const p of probes ?? []) {
      const arr = map.get(p.model_id) ?? [];
      arr.push(p);
      map.set(p.model_id, arr);
    }
    return map;
  }, [probes]);

  // Per model, keep only current-health probe rows (see effectiveProbes).
  const effectiveProbesByModel = useMemo(() => {
    const map = new Map<string, ProbeResult[]>();
    for (const [id, rows] of probeByName) map.set(id, effectiveProbes(rows));
    return map;
  }, [probeByName]);

  const rows = useMemo(
    () => buildRows(models, rh, ma, channelName),
    [models, rh, ma, channelName],
  );

  const firstWithTraffic = rows.find((r) => r.requests > 0)?.name ?? rows[0]?.name ?? null;
  useEffect(() => {
    if (selectedName === null && firstWithTraffic) setSelectedName(firstWithTraffic);
  }, [selectedName, firstWithTraffic]);
  const selected = rows.find((r) => r.name === selectedName) ?? rows[0] ?? null;

  const filtered = useMemo(
    () =>
      rows.filter(
        (r) =>
          !search ||
          r.name.toLowerCase().includes(search.toLowerCase()) ||
          r.channelNames.toLowerCase().includes(search.toLowerCase()),
      ),
    [rows, search],
  );

  // 24h cache hit ratio (input tokens served from cache)
  const cachePct = useMemo(() => {
    const inTok = (ma ?? []).reduce((s, m) => s + m.prompt_tokens + m.cache_hit_tokens, 0);
    const hit = (ma ?? []).reduce((s, m) => s + m.cache_hit_tokens, 0);
    return inTok > 0 ? +((hit / inTok) * 100).toFixed(1) : null;
  }, [ma]);

  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const successRate24h = agg?.success_rate_24h ?? 0;
  const p95 = funnel?.p95_latency ?? agg?.avg_latency_ms_24h ?? 0;
  const healthyCount = rows.filter((r) => r.health === 'good').length;
  const leadTitle = rows.some((r) => r.health === 'bad')
    ? t('monitor.overallDown')
    : rows.some((r) => r.health === 'warn')
      ? t('monitor.overallDegraded')
      : t('monitor.overallStable');

  const blocked = (funnel?.auth_fail_count ?? 0) + (funnel?.rate_limit_count ?? 0);
  const upstreamTotal = Math.max(
    0,
    (funnel?.total ?? 0) - blocked - (funnel?.bad_request_count ?? 0) - (funnel?.other_error_count ?? 0),
  );

  const incidents = useMemo(() => {
    const list: { key: string; kind: 'red' | 'amber' | 'blue'; title: string; meta: string }[] = [];
    for (const r of rows) {
      for (const ch of r.channels) {
        if (ch.requests > 0 && !ch.circuit_ok && ch.circuit_enabled) {
          list.push({
            key: `cb-${r.name}-${ch.channel_id}`,
            kind: 'red',
            title: `${r.name} · ${ch.channel_name || ch.channel_id}`,
            meta: t('monitor.circuitBroken'),
          });
        }
      }
    }
    for (const r of rows) {
      for (const p of effectiveProbesByModel.get(r.id) ?? []) {
        if (p.success) continue;
        list.push({
          key: `pf-${p.id}`,
          kind: 'amber',
          title: `${r.name} · ${channelName.get(p.channel_id) ?? p.channel_id}`,
          meta: `${t('monitor.probeFailed')} · ${new Date(p.probed_at).toLocaleString()}`,
        });
      }
    }
    return list.slice(0, 8);
  }, [rows, effectiveProbesByModel, channelName, t]);

  const chartData = useMemo(
    () =>
      (ua ?? []).map((d) => ({
        time: fmtHour(d.date),
        count: d.count,
        tokens: d.total_tokens,
        latency: d.latency_ms,
        error: d.count > 0 ? +((1 - d.success_count / d.count) * 100).toFixed(2) : 0,
      })),
    [ua],
  );

  return (
    <div className="space-y-4">
      <style>{`
        .mon-link i {
          position: absolute; top: -2.5px; left: 0; width: 6px; height: 6px;
          border-radius: 50%; background: hsl(var(--primary));
          box-shadow: 0 0 8px hsl(var(--primary));
          animation: mon-travel 2.2s linear infinite;
        }
        @keyframes mon-travel {
          from { left: 0; }
          to { left: calc(100% - 6px); }
        }
      `}</style>

      <StatusStrip
        leadTitle={leadTitle}
        leadCopy={t('monitor.leadCopy', { total: rows.length, healthy: healthyCount })}
        availability={successRate24h}
        currentRequests={totalCount || funnel?.total || 0}
        p95={p95}
        totalTokens={totalTokens24h}
        cachePct={cachePct}
        connected={connected}
      />

      <div className="grid gap-4 lg:grid-cols-[238px_minmax(0,1fr)_300px] items-start">
        <ModelCatalog
          rows={filtered}
          search={search}
          onSearch={setSearch}
          selectedName={selectedName}
          onSelect={setSelectedName}
        />

        <div className="grid gap-4 min-w-0">
          <PerformanceChart data={chartData} />
          <RequestFlow
            ingress={totalCount || funnel?.total || 0}
            directPass={successRate24h}
            upstream={upstreamTotal}
            rl={funnel?.rate_limit_count ?? 0}
            af={funnel?.auth_fail_count ?? 0}
            cachePct={cachePct}
            p95={p95}
            timeout={funnel?.timeout_count ?? 0}
            timeline={timeline}
          />
          <ChannelEndpointStatus row={selected} channelName={channelName} endpointUrl={endpointUrl} />
          <ModelCompareTable rows={rows} selectedName={selectedName} onSelect={setSelectedName} />
        </div>

        <div className="grid gap-4">
          <ModelInspector row={selected} />
          <IncidentList incidents={incidents} />
        </div>
      </div>
    </div>
  );
}
