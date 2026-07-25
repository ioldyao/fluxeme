import { useState, useMemo, useRef, useEffect, useCallback } from 'react';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useUsageFunnel, useUsageAggregate } from '@/api/usage';
import { useModels } from '@/api/models';
import { useChannels } from '@/api/channels';
import { fetchRoutingFlowSnapshot } from '@/api/routing';
import type { Channel, Model } from '@/types';

// ── design tokens ──────────────────────────────────────────────────
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

// ── Animated counter ───────────────────────────────────────────────
function AnimatedNumber({ value, style }: { value: number; style?: React.CSSProperties }) {
  const prevRef = useRef(value);
  const [display, setDisplay] = useState(value);
  useEffect(() => {
    if (value === prevRef.current) return;
    const start = prevRef.current;
    const duration = 300;
    const t0 = performance.now();
    let raf = 0;
    function tick(now: number) {
      const p = Math.min(1, (now - t0) / duration);
      setDisplay(Math.round(start + (value - start) * (1 - Math.pow(1 - p, 3))));
      if (p < 1) raf = requestAnimationFrame(tick);
    }
    raf = requestAnimationFrame(tick);
    prevRef.current = value;
    return () => cancelAnimationFrame(raf);
  }, [value]);
  return <span style={{ ...style, fontVariantNumeric: 'tabular-nums' }}>{display.toLocaleString()}</span>;
}

// ── Build topology ────────────────────────────────────────────────
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

// ── Real-time routing stream ──────────────────────────────────────
function useLiveCounts(topology: TopoModel[]) {
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [totalCount, setTotalCount] = useState(0);
  const [connected, setConnected] = useState(false);
  const [reconnectIn, setReconnectIn] = useState(0);
  const [pulseEvent, setPulseEvent] = useState<{ model: string; channel: string; ts: number } | null>(null);
  const topoRef = useRef(topology);
  topoRef.current = topology;

  useEffect(() => {
    fetchRoutingFlowSnapshot().then(snap => {
      if (Object.keys(snap).length === 0) return;
      setCounts(snap);
      const total = Object.entries(snap).filter(([k]) => k.split('>').length === 1).reduce((s, [, v]) => s + v, 0);
      setTotalCount(total);
    }).catch(() => {});
  }, []);

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
        const m = topo.find(t => t.model === ev.model || (t.pattern !== '*' && ev.model.startsWith(t.pattern.replace('*', ''))));
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

// ── Comet pulse for SVG river paths ────────────────────────────────
function RiverPulse({ pathD, onDone }: { pathD: string; onDone: () => void }) {
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
    circle.setAttribute('r', '4');
    circle.setAttribute('fill', '#267b7b');
    svg.appendChild(circle);
    const start = performance.now();
    const duration = 700;
    let raf = 0;
    function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const pt = pathEl!.getPointAtLength(t * len);
      circle.setAttribute('cx', String(pt.x));
      circle.setAttribute('cy', String(pt.y));
      circle.setAttribute('opacity', String(1 - t * 0.4));
      if (t < 1) raf = requestAnimationFrame(step);
      else { circle.remove(); doneRef.current(); }
    }
    raf = requestAnimationFrame(step);
    return () => { cancelAnimationFrame(raf); circle.remove(); };
  }, [pathD]);
  return (<g ref={svgRef}><path d={pathD} fill="none" stroke="none" /></g>);
}

// ── Timeline scrub ────────────────────────────────────────────────
function TimelineScrub({ aggregates }: {
  aggregates: { date: string; count: number; total_tokens: number }[];
}) {
  const [pos, setPos] = useState(24);
  const factor = 0.55 + 0.45 * Math.sin((pos / 24) * Math.PI);
  const peak = aggregates.length > 0 ? Math.max(...aggregates.map(d => d.count)) : 0;
  return (
    <div>
      <div className="flex justify-between text-[11px] text-muted-foreground mb-1.5">
        <span>24 小时流量回放</span>
        <span>{pos === 24 ? '现在' : `${String(pos).padStart(2, '0')}:00 · 历史回放`}</span>
      </div>
      <input type="range" min={0} max={24} value={pos} step={1}
        onChange={e => setPos(Number(e.target.value))}
        className="w-full accent-[var(--chart-1)]" />
      <div className="flex justify-between gap-3 text-[11px] text-muted-foreground mt-1">
        <span>00:00</span><span>06:00</span><span>12:00</span><span>18:00</span><span>现在</span>
      </div>
      <div className="flex gap-4 flex-wrap mt-3 text-[11px] text-muted-foreground">
        <span><i className="inline-block w-5 h-2 rounded-sm mr-1 align-middle" style={{ background: 'var(--chart-1)' }} />峰值 {fmtCount(Math.round(peak * factor))} req</span>
      </div>
    </div>
  );
}

// ── Main page ─────────────────────────────────────────────────────
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

  // ── derived dashboard data ────────────────────────────────────
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

  // Model→Channel count map for SVG river widths
  const modelChCounts = useMemo(() => {
    const m: Record<string, number> = {};
    topology.forEach(t => { m[t.model] = counts[keyFor(t.model)] || 0; });
    return m;
  }, [topology, counts]);

  const chReqMap = useMemo(() => {
    const m: Record<string, number> = {};
    topology.forEach(t => {
      t.channels.forEach(c => {
        const k = keyFor(t.model, c.id);
        m[k] = counts[k] || 0;
      });
    });
    return m;
  }, [topology, counts]);

  const maxModelCount = Math.max(...Object.values(modelChCounts), 1);
  const sortedModels = useMemo(() =>
    topology.slice().sort((a, b) => (counts[keyFor(b.model)] || 0) - (counts[keyFor(a.model)] || 0)),
    [topology, counts]
  );
  const topModels = sortedModels.slice(0, 5);

  // Collect all unique channels across all models (for right-side provider list)
  const allChannelReqs = useMemo(() => {
    const sum: Record<string, { name: string; count: number }> = {};
    topology.forEach(t => {
      t.channels.forEach(c => {
        const cnt = counts[keyFor(t.model, c.id)] || 0;
        if (sum[c.id]) sum[c.id].count += cnt;
        else sum[c.id] = { name: c.name, count: cnt };
      });
    });
    return Object.entries(sum)
      .map(([id, v]) => ({ id, name: v.name, count: v.count }))
      .sort((a, b) => b.count - a.count);
  }, [topology, counts]);
  const maxChCount = Math.max(...allChannelReqs.map(c => c.count), 1);

  // Pulse animation state for river bands
  const [pulses, setPulses] = useState<{ id: string; pathD: string }[]>([]);
  const prevTsRef = useRef(0);
  const pulseCooldown = useRef<Record<string, number>>({});
  const COOLDOWN = 400;

  // Generate SVG path for a model band (left edge → gate center)
  const modelBandPath = useCallback((index: number, total: number, _count: number) => {
    const y = 60 + index * (380 / Math.max(total, 1));
    const ty = 190 + (index - (total - 1) / 2) * 12;
    return `M0,${y} C240,${y + 5} 400,${140 + index * 8} 620,${ty} C670,${ty + 5} 700,${ty + 8} 718,${ty + 10}`;
  }, []);

  // Generate SVG path from gate to provider
  const providerBandPath = useCallback((index: number, total: number) => {
    const ty = 120 + index * (260 / Math.max(total, 1));
    return `M770,${ty + 10} C850,${ty + 5} 950,${ty - 10} 1200,${ty - 20}`;
  }, []);

  // Pulse on WebSocket event
  useEffect(() => {
    if (!pulseEvent || pulseEvent.ts === prevTsRef.current) return;
    prevTsRef.current = pulseEvent.ts;
    const { model, channel, ts } = pulseEvent;
    const mi = topModels.findIndex(t => t.model === model);
    if (mi >= 0) {
      const bandKey = `model-${model}`;
      const last = pulseCooldown.current[bandKey] || 0;
      if (ts - last >= COOLDOWN) {
        pulseCooldown.current[bandKey] = ts;
        const cnt = modelChCounts[model] || 1;
        const d = modelBandPath(mi, Math.max(topModels.length, 1), cnt);
        setPulses(prev => [...prev, { id: `${ts}-${model}`, pathD: d }]);
      }
    }
    // provider pulse
    const ci = allChannelReqs.findIndex(c => {
      const found = topology.find(t => t.model === model);
      return found?.channels.some(ch => ch.id === channel);
    });
    if (ci >= 0) {
      const bandKey = `ch-${channel}`;
      const last = pulseCooldown.current[bandKey] || 0;
      if (ts - last >= COOLDOWN) {
        pulseCooldown.current[bandKey] = ts;
        const d = providerBandPath(ci, Math.max(allChannelReqs.length, 1));
        setPulses(prev => [...prev, { id: `${ts}-${channel}`, pathD: d }]);
      }
    }
  }, [pulseEvent, topModels, modelChCounts, allChannelReqs, topology, modelBandPath, providerBandPath]);

  const removePulse = useCallback((id: string) => setPulses(prev => prev.filter(p => p.id !== id)), []);

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
            {connected ? 'LIVE' : reconnectIn > 0 ? `${reconnectIn}s` : '离线'}
          </span>
        </div>
        <div className="flex items-center gap-5 flex-wrap text-muted-foreground">
          <span>请求 <strong className="text-foreground tabular-nums">{fmtCount(totalCount || funnelTotal)}</strong></span>
          <span>Token <strong className="text-foreground tabular-nums">{fmtTokens(totalTokens24h)}</strong></span>
          <span>QPS <strong className="text-foreground tabular-nums">{qps.toFixed(1)}</strong></span>
        </div>
      </div>

      {/* ═══ RIVER FLOW ═══ */}
      <div className="relative py-5">
        <div className="grid grid-cols-3 text-xs text-muted-foreground mb-1">
          <div>模型入口</div>
          <div className="text-center">网关闸门</div>
          <div className="text-right">供应商出口</div>
        </div>

        <section className="relative h-[480px] border rounded-xl bg-card/40 overflow-hidden">
          {/* SVG river */}
          <svg viewBox="0 0 1200 480" preserveAspectRatio="none" className="absolute inset-0 w-full h-full">
            <defs>
              <linearGradient id="rw1" x1="0" x2="1">
                <stop offset="0%" stopColor="#7fc1b5" stopOpacity="0.85" />
                <stop offset="100%" stopColor="#267b7b" stopOpacity="0.6" />
              </linearGradient>
              <linearGradient id="rw2" x1="0" x2="1">
                <stop offset="0%" stopColor="#a8d5c9" stopOpacity="0.75" />
                <stop offset="100%" stopColor="#4f9b8e" stopOpacity="0.5" />
              </linearGradient>
            </defs>

            {/* Faint background grid */}
            <g opacity="0.15">
              <line x1="400" y1="0" x2="400" y2="480" stroke="#688082" strokeWidth="0.5" />
              <line x1="800" y1="0" x2="800" y2="480" stroke="#688082" strokeWidth="0.5" />
            </g>

            {/* Model→Gateway bands */}
            {topModels.map((m, i) => {
              const cnt = modelChCounts[m.model] || 0;
              const w = Math.max(6, Math.min(40, 8 + (cnt / maxModelCount) * 32));
              const grad = i % 2 === 0 ? 'url(#rw1)' : 'url(#rw2)';
              const y = 55 + i * 75;
              const ty = 190 + (i - (Math.max(topModels.length, 1) - 1) / 2) * 15;
              return (
                <path key={m.model}
                  d={`M0,${y} C240,${y + 5} 400,${140 + i * 6} 620,${ty} C670,${ty + 3} 700,${ty + 5} 720,${ty + 6}`}
                  fill="none" stroke={grad} strokeWidth={w} strokeLinecap="round" opacity={0.85} />
              );
            })}

            {/* Intercepted/dropped trickle (dashed) */}
            {(blocked + upstreamErrTotal) > 0 && (
              <path d="M0,440 C260,440 380,380 560,350"
                fill="none" stroke="#c65d50" strokeWidth="5" strokeDasharray="8 6" strokeLinecap="round" opacity="0.7" />
            )}

            {/* Gateway node */}
            <ellipse cx="600" cy="240" rx="160" ry="200" fill="rgba(38,123,123,0.04)" />
            <rect x="540" y="195" width="120" height="90" rx="16" fill="rgba(255,255,255,0.92)" stroke="#267b7b" strokeWidth="1.5" opacity="0.9" />

            {/* Gateway→Provider bands */}
            {allChannelReqs.slice(0, 3).map((ch, i) => {
              const w = Math.max(6, Math.min(36, 8 + (ch.count / maxChCount) * 28));
              const grad = i === 0 ? 'url(#rw1)' : 'url(#rw2)';
              const ty = 160 + i * 80;
              return (
                <path key={ch.id}
                  d={`M780,${ty} C880,${ty - 5} 980,${ty - 12} 1200,${ty - 18}`}
                  fill="none" stroke={grad} strokeWidth={w} strokeLinecap="round" opacity={0.75} />
              );
            })}

            {/* Pulse comets */}
            {pulses.map(p => <RiverPulse key={p.id} pathD={p.pathD} onDone={() => removePulse(p.id)} />)}
          </svg>

          {/* ═══ OVERLAY LABELS ═══ */}

          {/* Model labels (left) */}
          <div className="absolute inset-y-8 left-0 flex flex-col justify-around z-[3] pointer-events-none">
            {topModels.map(m => {
              const cnt = modelChCounts[m.model] || 0;
              const pct = funnelTotal > 0 ? (cnt / funnelTotal) * 100 : 0;
              return (
                <div key={m.model} className="pl-4">
                  <b className="text-xs leading-tight">{m.model.length > 16 ? `${m.model.slice(0, 14)}..` : m.model}</b>
                  <div className="text-[10px] text-muted-foreground tabular-nums">
                    {fmtCount(cnt)} req · {pct.toFixed(1)}%
                  </div>
                </div>
              );
            })}
            {topModels.length === 0 && !loading && (
              <div className="pl-4 text-[10px] text-muted-foreground">暂无模型数据</div>
            )}
          </div>

          {/* Gateway gate card */}
          <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-[160px] z-[4] pointer-events-none">
            <div className="bg-white/92 backdrop-blur rounded-xl border border-[rgba(38,123,123,0.3)] shadow-sm p-3.5 text-center">
              <div className="text-[9px] uppercase tracking-widest text-muted-foreground">GATEWAY</div>
              <div className="grid grid-cols-2 gap-x-3 gap-y-2 mt-2.5">
                <div><b className="text-base tabular-nums">{upstreamErrTotal + (funnel?.other_error_count ?? 0)}</b><div className="text-[9px] text-muted-foreground">异常拦截</div></div>
                <div><b className="text-base tabular-nums">{blocked}</b><div className="text-[9px] text-muted-foreground">业务限制</div></div>
                <div><b className="text-base tabular-nums">{availability.toFixed(1)}%</b><div className="text-[9px] text-muted-foreground">SLA</div></div>
                <div><b className="text-base tabular-nums">{qps.toFixed(1)}</b><div className="text-[9px] text-muted-foreground">QPS</div></div>
              </div>
            </div>
          </div>

          {/* Provider labels (right) */}
          <div className="absolute inset-y-8 right-0 flex flex-col justify-around z-[3] pointer-events-none text-right">
            {loading ? (
              <div className="pr-4 text-[10px] text-muted-foreground">加载中...</div>
            ) : allChannelReqs.length > 0 ? allChannelReqs.slice(0, 3).map(ch => (
              <div key={ch.id} className="pr-4">
                <b className="text-xs leading-tight">{ch.name}</b>
                <div className="text-[10px] text-muted-foreground tabular-nums">{fmtCount(ch.count)} req</div>
              </div>
            )) : (
              <div className="pr-4 text-[10px] text-muted-foreground">暂无渠道流量</div>
            )}
          </div>

          {/* Waterline metrics */}
          <div className="absolute left-[15%] right-[15%] bottom-3 flex justify-around text-[10px] text-muted-foreground z-[5]">
            <span>TTFT P50 <b className="text-foreground">{fmtLat(p50)}</b></span>
            <span>TTFT P99 <b className="text-foreground">{fmtLat(p99)}</b></span>
            <span>P95 <b className="text-foreground">{fmtLat(p95)}</b></span>
            <span>Max <b className="text-foreground">{fmtLat(funnel?.p99_latency ?? avgLat)}</b></span>
          </div>
        </section>
      </div>

      {/* ═══ TIMELINE ═══ */}
      <section className="pt-2 pb-4">
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
