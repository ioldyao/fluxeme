import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useModels } from '@/api/models';
import { useChannels } from '@/api/channels';
import { fetchRoutingFlowSnapshot } from '@/api/routing';
import type { Channel, Model } from '@/types';

// ── design tokens ──────────────────────────────────────────────────
const C = {
  bg: '#f5f5f3', cardBg: '#ffffff', border: '#e4e3de',
  line: '#d8d7d1', textPrimary: '#1a1a18', textSecondary: '#6b6a64',
  textMuted: '#9a988f', nodeBg: '#fafaf8', barTrack: '#eeede8',
  green: '#1a8a3d', low: '#4a7fc9', mid: '#d99a2b', high: '#c94a4a',
};
const LOAD_COLOR: Record<string, string> = { low: C.low, mid: C.mid, high: C.high };
const FONT_FAMILY = '-apple-system, PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

// ── types ───────────────────────────────────────────────────────────
interface TopoEndpoint { key: string; matchId: number | null; label: string; url: string }
interface TopoChannel { id: string; name: string; endpoints: TopoEndpoint[] }
interface TopoModel { model: string; pattern: string; channels: TopoChannel[] }
interface Pair { key: string; fromRef: React.RefObject<HTMLDivElement | null>; toRef: React.RefObject<HTMLDivElement | null> }
interface HopEvent { model: string; channel: string; endpoint: string | null; ts: number }

const keyFor = (...parts: (string | number)[]) => parts.join('>');

function loadClass(count: number, siblingCounts: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblingCounts);
  const ratio = count / max;
  if (ratio >= 0.66) return 'high';
  if (ratio >= 0.33) return 'mid';
  return 'low';
}

function matchPattern(text: string, pattern: string): boolean {
  if (pattern === '*') return true;
  if (!pattern.includes('*')) return text === pattern;
  const parts = pattern.split('*');
  if (parts.length === 2) {
    const [pfx, sfx] = parts;
    return (pfx === '' || text.startsWith(pfx)) && (sfx === '' || text.endsWith(sfx));
  }
  if (parts.length === 3) {
    const [pfx, mid, sfx] = parts;
    return text.startsWith(pfx) && text.includes(mid) && text.endsWith(sfx);
  }
  return pattern === text;
}

function resolveEvent(
  topology: TopoModel[],
  ev: { model: string; channel_id: string; endpoint_id?: number | null },
): { modelName: string; channelId: string; endpointKey: string | null } | null {
  const m = topology.find((t) => t.model === ev.model) || topology.find((t) => matchPattern(ev.model, t.pattern));
  if (!m) return null;
  const ch = m.channels.find((c) => c.id === ev.channel_id);
  if (!ch) return null;
  let ep: TopoEndpoint | undefined;
  if (ev.endpoint_id != null) ep = ch.endpoints.find((e) => e.matchId === ev.endpoint_id);
  if (!ep) ep = ch.endpoints[0];
  return { modelName: m.model, channelId: ch.id, endpointKey: ep ? ep.key : null };
}

function buildTopology(models: Model[], channels: Channel[]): TopoModel[] {
  const channelMap = new Map(channels.map((c) => [c.id, c]));
  const merged = new Map<string, TopoModel>();
  for (const m of models) {
    const key = m.name;
    let entry = merged.get(key);
    if (!entry) { entry = { model: m.name, pattern: m.model_pattern, channels: [] }; merged.set(key, entry); }
    for (const mc of m.channels) {
      const ch = channelMap.get(mc.channel_id);
      if (!ch || entry.channels.some((ec) => ec.id === ch.id)) continue;
      entry.channels.push({
        id: ch.id, name: ch.name || ch.id,
        endpoints: ch.endpoints.map((e, i) => ({
          key: e.id != null ? `id:${e.id}` : `${ch.id}#${i}`,
          matchId: e.id ?? null, label: `端点 ${i + 1}`, url: e.url,
        })),
      });
    }
  }
  return [...merged.values()];
}

// ── 1. Animated digits ──────────────────────────────────────────────
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
      const progress = Math.min(1, elapsed / duration);
      const eased = 1 - Math.pow(1 - progress, 3);
      setDisplay(Math.round(start + (end - start) * eased));
      if (progress < 1) raf = requestAnimationFrame(tick);
    }
    raf = requestAnimationFrame(tick);
    prevRef.current = value;
    return () => cancelAnimationFrame(raf);
  }, [value]);

  return <span style={{ ...style, fontVariantNumeric: 'tabular-nums' }}>{display.toLocaleString()}</span>;
}

// ── 5. Skeleton shimmer ─────────────────────────────────────────────
function SkeletonBar() {
  return (
    <div style={{
      marginTop: 6, height: 4, borderRadius: 2,
      background: 'linear-gradient(90deg, #eeede8 40%, #e0ded8 50%, #eeede8 60%)',
      backgroundSize: '200% 100%', animation: 'sk-shimmer 1.4s infinite linear',
    }} />
  );
}

// ── Pulse dot (native DOM drive, no React state per frame) ──────────
function CometPulse({ pathD, onDone }: { pathD: string; onDone: () => void }) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;

  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;

    // find the <path> element in the same <svg> to measure length
    const pathEl = svg.querySelector('path');
    if (!pathEl) return;
    const len = pathEl.getTotalLength();

    // create circle via DOM (bypasses React render for each frame)
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

    return () => {
      cancelAnimationFrame(raf);
      circle.remove();
    };
  }, [pathD]);

  return (
    <g ref={svgRef}>
      <path d={pathD} fill="none" stroke="none" />
    </g>
  );
}

// ── Connectors (unchanged core) ─────────────────────────────────────
function useConnectors(containerRef: React.RefObject<HTMLDivElement | null>, pairs: Pair[]) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [paths, setPaths] = useState<{ key: string; d: string }[]>([]);

  const recompute = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const cRect = container.getBoundingClientRect();
    const next = pairs
      .map(({ key, fromRef, toRef }) => {
        const fromEl = fromRef.current; const toEl = toRef.current;
        if (!fromEl || !toEl) return null;
        const fr = fromEl.getBoundingClientRect(); const tr = toEl.getBoundingClientRect();
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

// ── FlowNode ────────────────────────────────────────────────────────
function FlowNode({
  nodeRef, title, subtitle, count, loadCls, skeleton,
  pinged, showBar = true, barPct,
}: {
  nodeRef?: React.RefObject<HTMLDivElement | null>; title: string; subtitle?: string;
  count: number; loadCls?: 'low' | 'mid' | 'high' | null; skeleton?: boolean;
  pinged?: boolean; showBar?: boolean; barPct?: number;
}) {
  const color = loadCls ? LOAD_COLOR[loadCls] : null;
  const width = barPct != null ? barPct : loadCls === 'high' ? 100 : loadCls === 'mid' ? 60 : 25;

  return (
    <div ref={nodeRef} style={{
      borderRadius: 8, border: `1.5px solid ${color || C.border}`,
      background: C.nodeBg, padding: '9px 12px', fontSize: 12.5,
      transition: 'transform 150ms, border-color 300ms',
      transform: pinged ? 'scale(1.03)' : 'scale(1)',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
        <span style={{ fontWeight: 600, color: color || C.textPrimary }}>{title}</span>
        {skeleton
          ? <div style={{ width: 32, height: 14, borderRadius: 3, background: '#eeede8', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
          : <AnimatedNumber value={count} style={{ fontSize: 12, color: C.textSecondary }} />}
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: C.textMuted, marginTop: 2 }}>{subtitle}</div>}
      {showBar && !skeleton && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: C.barTrack, overflow: 'hidden' }}>
          <div style={{
            height: '100%', borderRadius: 2, width: `${loadCls ? width : 0}%`,
            background: color || 'transparent',
            transition: 'width 400ms ease, background-color 400ms ease',
          }} />
        </div>
      )}
      {showBar && skeleton && <SkeletonBar />}
    </div>
  );
}

// ── ModelPanel ──────────────────────────────────────────────────────
function ModelPanel({
  model, counts, lastEvent,
}: {
  model: TopoModel; counts: Record<string, number>;
  lastEvent: HopEvent | null;
}) {
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const modelNodeRef = useRef<HTMLDivElement>(null);
  const channelNodeRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  const endpointNodeRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  const [pulses, setPulses] = useState<{ id: string; pathD: string }[]>([]);
  const [pinged, setPinged] = useState<Record<string, boolean>>({});

  model.channels.forEach((c) => {
    if (!channelNodeRefs.current[c.id]) channelNodeRefs.current[c.id] = { current: null };
    c.endpoints.forEach((e) => {
      if (!endpointNodeRefs.current[e.key]) endpointNodeRefs.current[e.key] = { current: null };
    });
  });

  const connectorPairs = useMemo(() => {
    const pairs: Pair[] = [];
    model.channels.forEach((c) => {
      pairs.push({ key: keyFor(model.model, c.id), fromRef: modelNodeRef, toRef: channelNodeRefs.current[c.id] });
      c.endpoints.forEach((e) => {
        pairs.push({ key: keyFor(model.model, c.id, e.key), fromRef: channelNodeRefs.current[c.id], toRef: endpointNodeRefs.current[e.key] });
      });
    });
    return pairs;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model]);

  const { svgRef, paths } = useConnectors(containerRef, connectorPairs);

  // ── Hop-by-hop pulse animation ──────────────────────────────────
  // Each HopEvent fires exactly ONE pulse leg:
  //   endpoint == null  →  Hop 1: model → channel
  //   endpoint != null  →  Hop 2: channel → endpoint
  // The caller (useRoutingStream) emits the two hops with 500ms stagger.
  const prevTsRef = useRef(lastEvent?.ts);
  useEffect(() => {
    if (!lastEvent || lastEvent.model !== model.model) return;
    // Skip duplicate events (the useEffect may be called re-entrantly).
    if (lastEvent.ts === prevTsRef.current) return;
    prevTsRef.current = lastEvent.ts;
    const { channel, endpoint, ts } = lastEvent;

    if (endpoint) {
      // ── Hop 2: channel → endpoint ────────────────────────────
      const epPath = paths.find((p) => p.key === keyFor(model.model, channel, endpoint));
      if (epPath) setPulses((prev) => [...prev, { id: `${ts}-ep`, pathD: epPath.d }]);
      // Ping channel immediately, endpoint when pulse arrives (~400ms)
      const ck = keyFor(model.model, channel);
      const ek = keyFor(model.model, channel, endpoint);
      setPinged((prev) => ({ ...prev, [ck]: true }));
      setTimeout(() => setPinged((prev) => ({ ...prev, [ck]: false })), 300);
      setTimeout(() => {
        setPinged((prev) => ({ ...prev, [ek]: true }));
        setTimeout(() => setPinged((prev) => ({ ...prev, [ek]: false })), 300);
      }, 400);
    } else {
      // ── Hop 1: model → channel ───────────────────────────────
      const chPath = paths.find((p) => p.key === keyFor(model.model, channel));
      if (chPath) setPulses((prev) => [...prev, { id: `${ts}-ch`, pathD: chPath.d }]);
      // Ping model immediately, channel when pulse arrives (~400ms)
      const mk = keyFor(model.model);
      const ck = keyFor(model.model, channel);
      setPinged((prev) => ({ ...prev, [mk]: true }));
      setTimeout(() => setPinged((prev) => ({ ...prev, [mk]: false })), 300);
      setTimeout(() => {
        setPinged((prev) => ({ ...prev, [ck]: true }));
        setTimeout(() => setPinged((prev) => ({ ...prev, [ck]: false })), 300);
      }, 400);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastEvent]);

  const removePulse = useCallback((id: string) => {
    setPulses((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const modelCount = counts[keyFor(model.model)] || 0;
  const channelCounts = model.channels.map((c) => counts[keyFor(model.model, c.id)] || 0);
  const colLabelStyle: React.CSSProperties = {
    fontSize: 10.5, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.04em',
  };

  return (
    <div style={{ marginBottom: 16, borderRadius: 10, border: `1px solid ${C.border}`, background: C.cardBg, padding: '20px 24px' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 18, fontSize: 14, fontWeight: 600 }}>
        <span>{model.model}</span>
        <span style={{ fontSize: 11, fontWeight: 400, color: C.textMuted, background: '#f0efe9', padding: '1px 8px', borderRadius: 4, fontFamily: 'SF Mono, Consolas, monospace' }}>{model.pattern}</span>
        <span style={{ marginLeft: 'auto', fontSize: 12, fontWeight: 400, color: C.textSecondary }}>
          {t('routingFlow.reqCountPrefix')} <b style={{ color: C.textPrimary, fontWeight: 600 }}><AnimatedNumber value={modelCount} /></b>{' '}{t('routingFlow.reqCountSuffix')}
        </span>
      </div>

      <div ref={containerRef} style={{ position: 'relative', display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', alignItems: 'center', minHeight: 60 }}>
        <svg ref={svgRef} style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', overflow: 'visible', pointerEvents: 'none' }}>
          {paths.map((p) => <path key={p.key} d={p.d} fill="none" stroke={C.line} strokeWidth="1.5" />)}
          {pulses.map((pulse) => <CometPulse key={pulse.id} pathD={pulse.pathD} onDone={() => removePulse(pulse.id)} />)}
        </svg>

        <div style={{ zIndex: 1, gridColumn: 1, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colModel')}</div>
          <FlowNode nodeRef={modelNodeRef} title={model.model} count={modelCount} pinged={pinged[keyFor(model.model)]} showBar={false} />
        </div>
        <div />

        <div style={{ zIndex: 1, gridColumn: 3, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colChannel')}</div>
          {model.channels.map((c) => {
            const cnt = counts[keyFor(model.model, c.id)] || 0;
            const cls = loadClass(cnt, channelCounts);
            const sum = channelCounts.reduce((a, b) => a + b, 0) || 1;
            const k = keyFor(model.model, c.id);
            return <FlowNode key={c.id} nodeRef={channelNodeRefs.current[c.id]} title={c.name} count={cnt} loadCls={cls} barPct={Math.round((cnt / sum) * 100)} pinged={pinged[k]} />;
          })}
        </div>
        <div />

        <div style={{ zIndex: 1, gridColumn: 5, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colEndpoint')}</div>
          {model.channels.flatMap((c) => {
            const epCounts = c.endpoints.map((e) => counts[keyFor(model.model, c.id, e.key)] || 0);
            const esum = epCounts.reduce((a, b) => a + b, 0) || 1;
            return c.endpoints.map((e) => {
              const cnt = counts[keyFor(model.model, c.id, e.key)] || 0;
              const cls = loadClass(cnt, epCounts);
              const k = keyFor(model.model, c.id, e.key);
              return <FlowNode key={e.key} nodeRef={endpointNodeRefs.current[e.key]} title={e.label} subtitle={`${e.url} · ${c.name}`} count={cnt} loadCls={cls} barPct={Math.round((cnt / esum) * 100)} pinged={pinged[k]} />;
            });
          })}
        </div>
      </div>
    </div>
  );
}

// ── Skeleton panel (loading state) ──────────────────────────────────
function SkeletonPanel() {
  return (
    <div style={{ marginBottom: 16, borderRadius: 10, border: `1px solid ${C.border}`, background: C.cardBg, padding: '20px 24px' }}>
      <div style={{ height: 18, width: 180, borderRadius: 4, background: '#eeede8', marginBottom: 20, animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
      <div style={{ display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', gap: 24, minHeight: 60 }}>
        <div style={{ borderRadius: 8, border: `1.5px solid ${C.border}`, background: C.nodeBg, padding: '9px 12px' }}>
          <div style={{ height: 14, borderRadius: 3, background: '#eeede8', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
          <SkeletonBar />
        </div>
        <div /><div />
        <div />
        <div style={{ borderRadius: 8, border: `1.5px solid ${C.border}`, background: C.nodeBg, padding: '9px 12px' }}>
          <div style={{ height: 14, borderRadius: 3, background: '#eeede8', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
          <SkeletonBar />
        </div>
      </div>
    </div>
  );
}

// ── data hook ───────────────────────────────────────────────────────
function useRoutingStream(topology: TopoModel[]) {
  const [totalCount, setTotalCount] = useState(0);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [lastEvent, setLastEvent] = useState<HopEvent | null>(null);
  const [connected, setConnected] = useState(false);
  const [reconnectIn, setReconnectIn] = useState(0);
  const reconnectTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const topoRef = useRef(topology);
  topoRef.current = topology;
  // Coalesce same-path pulses: max 1 per unique route per COOLDOWN_MS.
  const pulseCooldown = useRef<Record<string, number>>({});
  const COOLDOWN_MS = 300;

  // Load 24h snapshot once on mount. Store raw data; apply + spread when
  // topology is available (avoids race: snapshot arriving before models/channels).
  const [rawSnapshot, setRawSnapshot] = useState<Record<string, number> | null>(null);
  useEffect(() => {
    fetchRoutingFlowSnapshot().then((snap) => {
      if (Object.keys(snap).length > 0) setRawSnapshot(snap);
    }).catch(() => {});
  }, []);

  // Merge snapshot into counts whenever topology or snapshot data changes
  useEffect(() => {
    if (!rawSnapshot || topology.length === 0) return;
    const patched = { ...rawSnapshot };
    for (const m of topology) {
      for (const c of m.channels) {
        const ck = keyFor(m.model, c.id);
        const chCount = patched[ck] || 0;
        const epSum = c.endpoints.reduce((s, e) => s + (patched[keyFor(m.model, c.id, e.key)] || 0), 0);
        if (chCount > epSum && c.endpoints.length > 0) {
          const missing = chCount - epSum;
          const each = Math.floor(missing / c.endpoints.length);
          let rem = missing - each * c.endpoints.length;
          for (const e of c.endpoints) {
            const ek = keyFor(m.model, c.id, e.key);
            patched[ek] = (patched[ek] || 0) + each + (rem > 0 ? 1 : 0);
            if (rem > 0) rem--;
          }
        }
      }
    }
    setCounts(patched);
    const total = Object.entries(patched).filter(([k]) => k.split('>').length === 1).reduce((s, [, v]) => s + v, 0);
    setTotalCount(total);
  }, [rawSnapshot, topology]);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;

    function connect() {
      const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${proto}://${window.location.host}/api/health/ws`);

      ws.onopen = () => { setConnected(true); setReconnectIn(0); if (reconnectTimer.current) { clearInterval(reconnectTimer.current); reconnectTimer.current = null; } };
      ws.onmessage = (e) => {
        let ev: { model?: string; channel_id?: string; endpoint_id?: number | null; latency_ms?: number };
        try { ev = JSON.parse(e.data); } catch { return; }
        if (!ev || typeof ev.model !== 'string' || typeof ev.channel_id !== 'string') return;
        const resolved = resolveEvent(topoRef.current, { model: ev.model, channel_id: ev.channel_id, endpoint_id: ev.endpoint_id });
        if (!resolved) return;
        const { modelName, channelId, endpointKey } = resolved;

        // RouteDecided (latency_ms == 0): count immediately, then pulse
        // hop-by-hop.  RequestCompleted (latency_ms > 0): silent; OTLP
        // has the full trace for retrospective inspection.
        const isDecided = ev.latency_ms === undefined || ev.latency_ms === 0;
        if (isDecided) {
          setCounts((prev) => {
            const next = { ...prev };
            next[keyFor(modelName)] = (next[keyFor(modelName)] || 0) + 1;
            next[keyFor(modelName, channelId)] = (next[keyFor(modelName, channelId)] || 0) + 1;
            if (endpointKey) next[keyFor(modelName, channelId, endpointKey)] = (next[keyFor(modelName, channelId, endpointKey)] || 0) + 1;
            return next;
          });
          setTotalCount((c) => c + 1);

          // ── Coalesced hop-by-hop pulses ────────────────────────
          // Same route within COOLDOWN_MS → share one pulse (1000 reqs → ~2-3 pulses).
          const now = performance.now();
          const hop1Key = `${modelName}>${channelId}>hop1`;
          const hop2Key = endpointKey ? `${modelName}>${channelId}>${endpointKey}>hop2` : '';
          const lastHop1 = pulseCooldown.current[hop1Key] || 0;

          if (now - lastHop1 >= COOLDOWN_MS) {
            pulseCooldown.current[hop1Key] = now;
            setLastEvent({ model: modelName, channel: channelId, endpoint: null, ts: now });
          }
          if (hop2Key) {
            const lastHop2 = pulseCooldown.current[hop2Key] || 0;
            if (now - lastHop2 >= COOLDOWN_MS) {
              pulseCooldown.current[hop2Key] = now;
              // Stagger hop 2 so it flies after hop 1 finishes
              const stagger = now - lastHop1 >= COOLDOWN_MS ? 500 : Math.max(200, 500 - (now - lastHop1));
              const hts = now + 1;
              setTimeout(() => {
                setLastEvent({ model: modelName, channel: channelId, endpoint: endpointKey, ts: hts });
              }, stagger);
            }
          }
        }
      };

      ws.onclose = () => {
        setConnected(false);
        if (!closed) {
          let c = 3;
          setReconnectIn(c);
          reconnectTimer.current = setInterval(() => {
            c--;
            if (c <= 0) { if (reconnectTimer.current) { clearInterval(reconnectTimer.current); reconnectTimer.current = null; } retry = setTimeout(connect, 500); }
            else setReconnectIn(c);
          }, 1000);
        }
      };
      ws.onerror = () => { try { ws?.close(); } catch { /* noop */ } };
    }

    connect();
    return () => { closed = true; if (retry) clearTimeout(retry); if (reconnectTimer.current) clearInterval(reconnectTimer.current); try { ws?.close(); } catch { /* noop */ } };
  }, []);

  return { counts, totalCount, lastEvent, connected, reconnectIn };
}

// ── page ────────────────────────────────────────────────────────────
export default function RoutingFlow() {
  const { t } = useTranslation();
  const { data: models, isLoading: mLoading } = useModels();
  const { data: channels, isLoading: cLoading } = useChannels();

  const topology = useMemo(() => {
    if (!models || !channels) return [];
    return buildTopology(models, channels).filter((m) => m.channels.length > 0);
  }, [models, channels]);

  const { counts, totalCount, lastEvent, connected, reconnectIn } = useRoutingStream(topology);
  const loading = mLoading || cLoading;

  return (
    <div style={{ fontFamily: FONT_FAMILY, color: C.textPrimary }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>{t('routingFlow.title')}</h1>
      <p style={{ fontSize: 13, color: C.textSecondary, margin: '0 0 20px' }}>
        {t('routingFlow.subtitle')}
        <span style={{ color: C.low }}> {t('routingFlow.legendLow')}</span> ·
        <span style={{ color: C.mid }}> {t('routingFlow.legendMid')}</span> ·
        <span style={{ color: C.high }}> {t('routingFlow.legendHigh')}</span>
      </p>

      <div style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: connected ? C.green : C.textMuted }}>
          <span style={{ width: 7, height: 7, borderRadius: '50%', background: connected ? C.green : C.textMuted, animation: connected ? 'rfl-pulse 1.6s infinite' : 'none' }} />
          {connected ? 'LIVE' : reconnectIn > 0 ? `⏳ ${reconnectIn}s` : t('routingFlow.connecting')}
        </div>
        <div style={{ fontSize: 12, color: C.textSecondary }}>
          {t('routingFlow.totalRequests')}{' '}
          <b style={{ fontSize: 15, color: C.textPrimary, fontWeight: 600 }}><AnimatedNumber value={totalCount} /></b>
        </div>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 16, fontSize: 11.5, color: C.textSecondary }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.low, display: 'inline-block' }} /> {t('routingFlow.loadLow')}</div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.mid, display: 'inline-block' }} /> {t('routingFlow.loadMid')}</div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.high, display: 'inline-block' }} /> {t('routingFlow.loadHigh')}</div>
        </div>
      </div>

      {/* 6. WS disconnect overlay */}
      <div style={{ position: 'relative' }}>
        {loading ? <><SkeletonPanel /><SkeletonPanel /></> : topology.length === 0 ? (
          <div style={{ borderRadius: 10, border: `1px dashed ${C.border}`, background: C.cardBg, padding: '40px 24px', textAlign: 'center', fontSize: 13, color: C.textSecondary }}>
            {t('routingFlow.empty')}
          </div>
        ) : topology.map((m) => <ModelPanel key={m.model} model={m} counts={counts} lastEvent={lastEvent} />)}

        {!loading && !connected && topology.length > 0 && (
          <div style={{ position: 'absolute', inset: 0, borderRadius: 10, background: 'rgba(255,255,255,0.55)', backdropFilter: 'blur(1px)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 10 }}>
            <span style={{ fontSize: 14, color: C.textSecondary, fontWeight: 500 }}>
              🔌 {t('routingFlow.connecting')}... {reconnectIn > 0 ? `(${reconnectIn}s)` : ''}
            </span>
          </div>
        )}
      </div>

      <style>{`
        @keyframes rfl-pulse {
          0% { box-shadow: 0 0 0 0 rgba(26,138,61,0.5); }
          70% { box-shadow: 0 0 0 6px rgba(26,138,61,0); }
          100% { box-shadow: 0 0 0 0 rgba(26,138,61,0); }
        }
        @keyframes sk-shimmer {
          0% { background-position: 200% 0; }
          100% { background-position: -200% 0; }
        }
      `}</style>
    </div>
  );
}
