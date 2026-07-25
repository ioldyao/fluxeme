import { useState, useMemo, useRef, useEffect, useCallback } from 'react';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useUsageFunnel, useUsageAggregate } from '@/api/usage';
import { useModels } from '@/api/models';
import { useChannels } from '@/api/channels';
import { fetchRoutingFlowSnapshot } from '@/api/routing';
import type { Channel, Model } from '@/types';

// ── design tokens ──────────────────────────────────────────────────
const LOAD_COLORS = { low: '#4a7fc9', mid: '#d99a2b', high: '#c94a4a' };
const keyFor = (...parts: (string | number)[]) => parts.join('>');

// ── helpers ─────────────────────────────────────────────────────
function fmtLat(ms: number) {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  return `${ms.toFixed(0)}ms`;
}
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

// ── Animated counter ───────────────────────────────────────────
function AnimatedNumber({ value, style }: { value: number; style?: React.CSSProperties }) {
  const prevRef = useRef(value);
  const [display, setDisplay] = useState(value);
  useEffect(() => {
    if (value === prevRef.current) return;
    const start = prevRef.current;
    const end = value;
    const duration = 300;
    const t0 = performance.now();
    let raf = 0;
    function tick(now: number) {
      const elapsed = now - t0;
      const p = Math.min(1, elapsed / duration);
      setDisplay(Math.round(start + (end - start) * (1 - Math.pow(1 - p, 3))));
      if (p < 1) raf = requestAnimationFrame(tick);
    }
    raf = requestAnimationFrame(tick);
    prevRef.current = value;
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return <span style={{ ...style, fontVariantNumeric: 'tabular-nums' }}>{display.toLocaleString()}</span>;
}

// ── Skeleton ───────────────────────────────────────────────────
function SkeletonBar() {
  return (
    <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: '#e5e4e0', overflow: 'hidden' }}>
      <div style={{ width: '40%', height: '100%', borderRadius: 2, background: '#d0cfca', animation: 'sk-shimmer 1.4s infinite' }} />
    </div>
  );
}

// ── FlowNode (from RoutingFlow) ────────────────────────────────
function FlowNode({
  title, subtitle, count, loadCls, skeleton, barPct, pinged,
}: {
  title: string; subtitle?: string; count: number;
  loadCls?: 'low' | 'mid' | 'high' | null; skeleton?: boolean; barPct?: number; pinged?: boolean;
}) {
  const color = loadCls ? LOAD_COLORS[loadCls] : null;
  const w = barPct != null ? barPct : loadCls === 'high' ? 100 : loadCls === 'mid' ? 60 : 25;
  return (
    <div style={{
      borderRadius: 8, border: `1.5px solid ${color || '#d8d7d1'}`,
      background: '#fafaf8', padding: '9px 12px', fontSize: 12.5,
      transition: 'transform 150ms, border-color 300ms',
      transform: pinged ? 'scale(1.03)' : 'scale(1)',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', gap: 8 }}>
        <span style={{ fontWeight: 600, color: color || '#1a1a18', fontSize: 12.5, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{title}</span>
        {skeleton
          ? <div style={{ width: 32, height: 14, borderRadius: 3, background: '#eeede8' }} />
          : <AnimatedNumber value={count} style={{ fontSize: 12, color: '#6b6a64', whiteSpace: 'nowrap' }} />}
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: '#9a988f', marginTop: 2 }}>{subtitle}</div>}
      {!skeleton && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: '#eeede8', overflow: 'hidden' }}>
          <div style={{ height: '100%', borderRadius: 2, width: `${loadCls ? w : 0}%`, background: color || 'transparent', transition: 'width 400ms ease' }} />
        </div>
      )}
      {skeleton && <SkeletonBar />}
    </div>
  );
}

// ── Comet pulse ───────────────────────────────────────────────
function CometPulse({ pathD, onDone }: { pathD: string; onDone: () => void }) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    const pathEl = svg.querySelector('path');
    if (!pathEl) return;
    const len = pathEl.getTotalLength();
    const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    circle.setAttribute('r', '3.5');
    circle.setAttribute('fill', '#4a7fc9');
    svg.appendChild(circle);
    const start = performance.now();
    const duration = 650;
    let raf = 0;
    function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const pt = pathEl!.getPointAtLength(t * len);
      circle.setAttribute('cx', String(pt.x));
      circle.setAttribute('cy', String(pt.y));
      circle.setAttribute('opacity', String(1 - t * 0.3));
      if (t < 1) raf = requestAnimationFrame(step);
      else { circle.remove(); doneRef.current(); }
    }
    raf = requestAnimationFrame(step);
    return () => { cancelAnimationFrame(raf); circle.remove(); };
  }, [pathD]);
  return (<g ref={svgRef}><path d={pathD} fill="none" stroke="none" /></g>);
}

// ── Connectors hook ───────────────────────────────────────────
function useConnectors(containerRef: React.RefObject<HTMLDivElement | null>, pairs: { key: string; fromEl: HTMLElement | null; toEl: HTMLElement | null }[]) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [paths, setPaths] = useState<{ key: string; d: string }[]>([]);
  const recompute = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const cRect = container.getBoundingClientRect();
    const next = pairs
      .map(({ key, fromEl, toEl }) => {
        if (!fromEl || !toEl) return null;
        const fr = fromEl.getBoundingClientRect();
        const tr = toEl.getBoundingClientRect();
        const p0 = { x: fr.right - cRect.left, y: fr.top + fr.height / 2 - cRect.top };
        const p1 = { x: tr.left - cRect.left, y: tr.top + tr.height / 2 - cRect.top };
        const midX = (p0.x + p1.x) / 2;
        return { key, d: `M ${p0.x} ${p0.y} C ${midX} ${p0.y}, ${midX} ${p1.y}, ${p1.x} ${p1.y}` };
      })
      .filter((v): v is { key: string; d: string } => !!v);
    setPaths(next);
  }, [containerRef, pairs]);
  useEffect(() => {
    recompute();
    const ro = new ResizeObserver(recompute);
    if (containerRef.current) ro.observe(containerRef.current);
    window.addEventListener('resize', recompute);
    return () => { ro.disconnect(); window.removeEventListener('resize', recompute); };
  }, [recompute, containerRef]);
  return { svgRef, paths };
}

// ── Timeline scrub ──────────────────────────────────────────────
function TimelineScrub({ aggregates }: {
  aggregates: { date: string; count: number; total_tokens: number }[];
}) {
  const [pos, setPos] = useState(24);
  const factor = 0.55 + 0.45 * Math.sin((pos / 24) * Math.PI);
  const timeLabel = pos === 24 ? '现在' : `${String(pos).padStart(2, '0')}:00 · 历史回放`;
  const peak = aggregates.length > 0 ? Math.max(...aggregates.map(d => d.count)) : 0;
  return (
    <div>
      <div className="flex justify-between text-[11px] text-muted-foreground mb-1.5">
        <span>24 小时流量回放</span>
        <span>{timeLabel}</span>
      </div>
      <input type="range" min={0} max={24} value={pos} step={1}
        onChange={e => setPos(Number(e.target.value))}
        className="w-full accent-[var(--chart-1)]" />
      <div className="flex justify-between gap-3 text-[11px] text-muted-foreground mt-1">
        <span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>现在</span>
      </div>
      <div className="flex gap-4 flex-wrap mt-3 text-[11px] text-muted-foreground">
        <span><i className="inline-block w-5 h-2 rounded-sm mr-1 align-middle" style={{ background: 'var(--chart-1)' }} />峰值 {fmtCount(Math.round(peak * factor))} req</span>
        <span><i className="inline-block w-5 h-[3px] rounded-sm mr-1 align-middle" style={{ background: 'var(--chart-2)' }} />TPS</span>
        <span className="text-muted-foreground/60">当前滑块可拖动查看历史时刻</span>
      </div>
    </div>
  );
}

// ── Build topology ────────────────────────────────────────────
interface TopoChannel { id: string; name: string }
interface TopoModel { model: string; pattern: string; channels: TopoChannel[] }

function buildTopology(models: Model[], channels: Channel[]): TopoModel[] {
  const channelMap = new Map(channels.map(c => [c.id, c]));
  const merged = new Map<string, TopoModel>();
  for (const m of models) {
    const key = m.name;
    let entry = merged.get(key);
    if (!entry) { entry = { model: m.name, pattern: m.model_pattern, channels: [] }; merged.set(key, entry); }
    for (const mc of m.channels) {
      const ch = channelMap.get(mc.channel_id);
      if (!ch || entry.channels.some(ec => ec.id === ch.id)) continue;
      entry.channels.push({ id: ch.id, name: ch.name || ch.id });
    }
  }
  return [...merged.values()];
}

function loadClass(count: number, siblings: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblings);
  const ratio = count / max;
  if (ratio >= 0.66) return 'high';
  if (ratio >= 0.33) return 'mid';
  return 'low';
}

// ── Real-time routing stream (simplified from RoutingFlow) ───
function useLiveCounts(topology: TopoModel[]) {
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [totalCount, setTotalCount] = useState(0);
  const [connected, setConnected] = useState(false);
  const [reconnectIn, setReconnectIn] = useState(0);
  const [pulseEvent, setPulseEvent] = useState<{ model: string; channel: string; ts: number } | null>(null);
  const topoRef = useRef(topology);
  topoRef.current = topology;

  // Load 24h snapshot once
  useEffect(() => {
    fetchRoutingFlowSnapshot().then(snap => {
      if (Object.keys(snap).length === 0) return;
      setCounts(snap);
      const total = Object.entries(snap)
        .filter(([k]) => k.split('>').length === 1)
        .reduce((s, [, v]) => s + v, 0);
      setTotalCount(total);
    }).catch(() => {});
  }, []);

  // WebSocket live events
  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    function connect() {
      const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${proto}://${window.location.host}/api/health/ws`);
      ws.onopen = () => { setConnected(true); setReconnectIn(0); };
      ws.onmessage = (e) => {
        let ev: any;
        try { ev = JSON.parse(e.data); } catch { return; }
        if (!ev || typeof ev.model !== 'string' || typeof ev.channel_id !== 'string') return;
        const topo = topoRef.current;
        const m = topo.find(t => t.model === ev.model) || topo.find(t => t.pattern === '*' || ev.model.startsWith(t.pattern.replace('*', '')));
        if (!m) return;
        const ch = m.channels.find(c => c.id === ev.channel_id);
        if (!ch) return;

        setCounts(prev => {
          const next = { ...prev };
          next[keyFor(m.model)] = (next[keyFor(m.model)] || 0) + 1;
          next[keyFor(m.model, ch.id)] = (next[keyFor(m.model, ch.id)] || 0) + 1;
          return next;
        });
        setTotalCount(c => c + 1);
        setPulseEvent({ model: m.model, channel: ch.id, ts: performance.now() });
      };
      ws.onclose = () => {
        setConnected(false);
        if (!closed) {
          let c = 3;
          setReconnectIn(c);
          const timer = setInterval(() => {
            c--;
            if (c <= 0) { clearInterval(timer); setTimeout(connect, 500); }
            else setReconnectIn(c);
          }, 1000);
        }
      };
      ws.onerror = () => { try { ws?.close(); } catch {} };
    }
    connect();
    return () => { closed = true; try { ws?.close(); } catch {} };
  }, []);

  return { counts, totalCount, connected, reconnectIn, pulseEvent };
}

// ── Main page ───────────────────────────────────────────────────
export default function FlowControlTower() {
  const [days] = useState(1);
  const { data: stats } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: funnel } = useUsageFunnel(days);
  const { data: ua } = useUsageAggregate(days);
  const { data: models, isLoading: mLoading } = useModels();
  const { data: channels, isLoading: cLoading } = useChannels();

  const topology = useMemo(() => {
    if (!models || !channels) return [];
    return buildTopology(models, channels).filter(m => m.channels.length > 0);
  }, [models, channels]);

  const { counts, totalCount, connected, reconnectIn, pulseEvent } = useLiveCounts(topology);
  const loading = mLoading || cLoading;

  // Topology refs for connectors
  const containerRef = useRef<HTMLDivElement>(null);
  const modelRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  const channelRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  topology.forEach(m => {
    if (!modelRefs.current[m.model]) modelRefs.current[m.model] = { current: null };
    m.channels.forEach(c => {
      if (!channelRefs.current[c.id]) channelRefs.current[c.id] = { current: null };
    });
  });

  // Connector pairs
  const connectorPairs = useMemo(() => {
    const pairs: { key: string; fromEl: HTMLElement | null; toEl: HTMLElement | null }[] = [];
    topology.forEach(m => {
      m.channels.forEach(c => {
        pairs.push({
          key: keyFor(m.model, c.id),
          fromEl: modelRefs.current[m.model]?.current,
          toEl: channelRefs.current[c.id]?.current,
        });
      });
    });
    return pairs;
  }, [topology]);

  const { svgRef, paths } = useConnectors(containerRef, connectorPairs);

  // Pulse tracks
  const [pulses, setPulses] = useState<{ id: string; pathD: string }[]>([]);
  const [pinged, setPinged] = useState<Record<string, boolean>>({});
  const prevTsRef = useRef(0);

  useEffect(() => {
    if (!pulseEvent || pulseEvent.ts === prevTsRef.current) return;
    prevTsRef.current = pulseEvent.ts;
    const { model, channel } = pulseEvent;
    const p = paths.find(pp => pp.key === keyFor(model, channel));
    if (p) {
      setPulses(prev => [...prev, { id: `${pulseEvent.ts}`, pathD: p.d }]);
      const mk = keyFor(model);
      const ck = keyFor(model, channel);
      setPinged(prev => ({ ...prev, [mk]: true }));
      setTimeout(() => setPinged(prev => ({ ...prev, [mk]: false })), 300);
      setTimeout(() => {
        setPinged(prev => ({ ...prev, [ck]: true }));
        setTimeout(() => setPinged(prev => ({ ...prev, [ck]: false })), 300);
      }, 400);
    }
  }, [pulseEvent, paths]);

  const removePulse = useCallback((id: string) => setPulses(prev => prev.filter(p => p.id !== id)), []);

  // ── derived dashboard data ──────────────────────────────────
  const availability = agg?.success_rate_24h ?? 0;
  const avgLat = agg?.avg_latency_ms_24h ?? 0;
  const requests24h = agg?.requests_24h ?? 0;
  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const modelCount = stats?.models ?? 0;
  const channelCount = stats?.channels ?? 0;

  const funnelTotal = funnel?.total ?? requests24h;
  const blocked = (funnel?.auth_fail_count ?? 0) + (funnel?.rate_limit_count ?? 0);
  const upstreamErrTotal = (funnel?.upstream_error_count ?? 0) + (funnel?.timeout_count ?? 0);
  const p99 = funnel?.p99_latency ?? avgLat;
  const p50 = funnel?.p50_latency ?? avgLat;
  const p95 = funnel?.p95_latency ?? avgLat;
  const qps = funnelTotal > 0 ? (funnelTotal / 86400) : 0;
  const tps = totalTokens24h > 0 ? (totalTokens24h / 86400) : 0;

  return (
    <div className="space-y-0">
      {/* ═══ TOWER HEADER ═══ */}
      <div className="flex items-center justify-between gap-5 py-3 px-0.5 border-b text-sm flex-wrap">
        <div className="flex items-center gap-5 flex-wrap">
          <span className="flex items-center gap-1.5">
            <span className={`w-2 h-2 rounded-full ${connected ? 'bg-emerald-600 animate-pulse shadow-[0_0_0_0_rgba(5,150,105,0.35)]' : 'bg-muted-foreground'}`} />
            <b>流控台</b>
          </span>
          <span className="text-muted-foreground tabular-nums">
            <strong className="text-foreground">{modelCount}</strong> 模型 · <strong className="text-foreground">{channelCount}</strong> 渠道
          </span>
          <span className="text-muted-foreground">
            可用性 <strong className={availability >= 99 ? 'text-emerald-700' : 'text-amber-700'}>{availability.toFixed(2)}%</strong>
          </span>
          <span className="text-[11px] font-mono tabular-nums flex items-center gap-1.5">
            <span className={`inline-block w-1.5 h-1.5 rounded-full ${connected ? 'bg-emerald-500' : 'bg-muted-foreground'}`} />
            {connected ? 'LIVE' : reconnectIn > 0 ? `重连 ${reconnectIn}s` : '离线'}
          </span>
        </div>
        <div className="flex items-center gap-5 flex-wrap text-muted-foreground">
          <span>请求 <strong className="text-foreground tabular-nums">{fmtCount(totalCount || funnelTotal)}</strong></span>
          <span>Token <strong className="text-foreground tabular-nums">{fmtTokens(totalTokens24h)}</strong></span>
          <span>TPS <strong className="text-foreground tabular-nums">{tps.toFixed(1)}</strong></span>
        </div>
      </div>

      {/* ═══ ROUTING TOPOLOGY ═══ */}
      <div className="py-5">
        <div ref={containerRef} className="relative" style={{ minHeight: 300 }}>
          {/* SVG connectors layer */}
          <svg ref={svgRef} className="absolute inset-0 w-full h-full pointer-events-none" style={{ overflow: 'visible' }}>
            {paths.map(p => <path key={p.key} d={p.d} fill="none" stroke="#d8d7d1" strokeWidth="1.5" />)}
            {pulses.map(pulse => <CometPulse key={pulse.id} pathD={pulse.pathD} onDone={() => removePulse(pulse.id)} />)}
          </svg>

          {loading ? (
            <>
              <div className="flex gap-8 items-center justify-center py-16">
                <div className="w-48"><FlowNode title="" count={0} skeleton /></div>
                <div className="text-muted-foreground text-xs text-center">加载中...</div>
                <div className="w-48"><FlowNode title="" count={0} skeleton /></div>
              </div>
            </>
          ) : topology.length === 0 ? (
            <div className="text-center py-20 text-sm text-muted-foreground">暂无拓扑数据 · 请先配置模型和渠道绑定</div>
          ) : (
            <div className="space-y-6">
              {topology.map(m => {
                const modelCnt = counts[keyFor(m.model)] || 0;
                const chCounts = m.channels.map(c => counts[keyFor(m.model, c.id)] || 0);
                return (
                  <div key={m.model} className="rounded-xl border bg-card p-5 shadow-sm">
                    {/* Model header row */}
                    <div className="flex items-center gap-3 mb-4">
                      <span className="font-semibold text-sm">{m.model}</span>
                      <span className="text-[10px] px-2 py-0.5 rounded bg-muted text-muted-foreground font-mono">{m.pattern}</span>
                      <span className="ml-auto text-xs text-muted-foreground tabular-nums">
                        共 <b className="text-foreground"><AnimatedNumber value={modelCnt} /></b> 次请求
                      </span>
                    </div>

                    {/* Model → Channels grid */}
                    <div className="grid grid-cols-[1fr_auto_1fr] gap-4 items-center">
                      {/* Model node (left) */}
                      <div ref={modelRefs.current[m.model]}>
                        <FlowNode title={m.model} count={modelCnt} pinged={pinged[keyFor(m.model)]} />
                      </div>

                      {/* Arrow */}
                      <div className="text-muted-foreground/40 text-xs select-none">→</div>

                      {/* Channel nodes (right) */}
                      <div className="space-y-2">
                        {m.channels.map(c => {
                          const cnt = counts[keyFor(m.model, c.id)] || 0;
                          const cls = loadClass(cnt, chCounts);
                          return (
                            <div key={c.id} ref={channelRefs.current[c.id]}>
                              <FlowNode
                                title={c.name}
                                count={cnt}
                                loadCls={cls}
                                barPct={chCounts.reduce((a, b) => a + b, 0) > 0 ? Math.round((cnt / Math.max(1, chCounts.reduce((a, b) => a + b, 0))) * 100) : 0}
                                pinged={pinged[keyFor(m.model, c.id)]}
                              />
                            </div>
                          );
                        })}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* Connection overlay */}
          {!loading && topology.length > 0 && !connected && (
            <div className="absolute inset-0 rounded-xl bg-background/55 backdrop-blur-[1px] flex items-center justify-center z-10">
              <span className="text-sm text-muted-foreground font-medium">
                🔌 连接断开{reconnectIn > 0 ? ` · ${reconnectIn}s 后重试` : ' · 重连中...'}
              </span>
            </div>
          )}
        </div>
      </div>

      {/* ═══ GATE STATS + WATERLINE ═══ */}
      <div className="grid grid-cols-[1fr_auto_1fr] gap-4 items-stretch py-4 border-y">
        {/* left: funnel stats */}
        <div className="flex flex-col gap-3">
          <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-mono">拦截统计</div>
          <div className="grid grid-cols-2 gap-2">
            <div className="border rounded-lg p-3 text-center">
              <b className="text-lg tabular-nums">{upstreamErrTotal + (funnel?.other_error_count ?? 0)}</b>
              <span className="block text-[10px] text-muted-foreground">异常拦截</span>
            </div>
            <div className="border rounded-lg p-3 text-center">
              <b className="text-lg tabular-nums">{blocked}</b>
              <span className="block text-[10px] text-muted-foreground">业务限制</span>
            </div>
            <div className="border rounded-lg p-3 text-center">
              <b className="text-lg tabular-nums">{availability.toFixed(1)}%</b>
              <span className="block text-[10px] text-muted-foreground">SLA</span>
            </div>
            <div className="border rounded-lg p-3 text-center">
              <b className="text-lg tabular-nums">{qps.toFixed(1)}</b>
              <span className="block text-[10px] text-muted-foreground">QPS</span>
            </div>
          </div>
        </div>

        {/* center: load legend */}
        <div className="flex flex-col justify-center px-6 border-x text-[11px] text-muted-foreground">
          <div className="space-y-2">
            <span className="flex items-center gap-2"><i className="inline-block w-5 h-[5px] rounded-sm" style={{ background: LOAD_COLORS.low }} />低负载</span>
            <span className="flex items-center gap-2"><i className="inline-block w-5 h-[5px] rounded-sm" style={{ background: LOAD_COLORS.mid }} />中负载</span>
            <span className="flex items-center gap-2"><i className="inline-block w-5 h-[5px] rounded-sm" style={{ background: LOAD_COLORS.high }} />高负载</span>
          </div>
          <div className="mt-4 text-xs">
            <div>TTFT P50 <b className="text-foreground">{fmtLat(p50)}</b></div>
            <div>TTFT P99 <b className="text-foreground">{fmtLat(p99)}</b></div>
            <div>请求 P95 <b className="text-foreground">{fmtLat(p95)}</b></div>
          </div>
        </div>

        {/* right: aggregate metrics */}
        <div className="flex flex-col gap-3">
          <div className="text-[10px] uppercase tracking-wider text-muted-foreground font-mono">流量概览</div>
          <div className="grid grid-cols-2 gap-2">
            <div className="border rounded-lg p-3">
              <div className="text-[10px] text-muted-foreground">请求总量</div>
              <b className="text-base tabular-nums">{fmtCount(funnelTotal)}</b>
            </div>
            <div className="border rounded-lg p-3">
              <div className="text-[10px] text-muted-foreground">Token 总量</div>
              <b className="text-base tabular-nums">{fmtTokens(totalTokens24h)}</b>
            </div>
            <div className="border rounded-lg p-3">
              <div className="text-[10px] text-muted-foreground">P99 延迟</div>
              <b className="text-base tabular-nums">{fmtLat(p99)}</b>
            </div>
            <div className="border rounded-lg p-3">
              <div className="text-[10px] text-muted-foreground">P50 延迟</div>
              <b className="text-base tabular-nums">{fmtLat(p50)}</b>
            </div>
          </div>
        </div>
      </div>

      {/* ═══ TIMELINE ═══ */}
      <section className="pt-4">
        {ua && ua.length > 0 ? <TimelineScrub aggregates={ua} /> : (
          <div className="text-xs text-muted-foreground text-center py-8">暂无时序数据</div>
        )}
      </section>

      <style>{`
        @keyframes sk-shimmer {
          0% { opacity: 0.4; }
          50% { opacity: 1; }
          100% { opacity: 0.4; }
        }
      `}</style>
    </div>
  );
}
