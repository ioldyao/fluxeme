import { useState, useRef, useEffect, useCallback, useMemo, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ShieldAlert, ShieldBan, ShieldCheck } from 'lucide-react';
import { usePublicModels } from '@fluxeme/shared/src/api/models';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { fetchRoutingFlowSnapshot } from '@fluxeme/shared/src/api/routing';

// ── design tokens ──────────────────────────────────────────────────
const C = {
  bg: 'var(--muted)', cardBg: 'var(--card)', border: 'var(--secondary)',
  line: 'var(--border)', textPrimary: 'var(--foreground)', textSecondary: 'var(--muted-foreground)',
  textMuted: 'var(--muted-foreground)', nodeBg: 'var(--muted)', barTrack: 'var(--secondary)',
  green: 'var(--chart-2)', low: 'var(--chart-1)', mid: 'var(--sidebar-primary)', high: 'var(--destructive)',
};
const HEAT_COLOR: Record<string, string> = { low: C.low, mid: C.mid, high: C.high };
const FONT_FAMILY = '-apple-system, PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

// ── topology helpers ────────────────────────────────────────────────
import { buildTopology, bindingKey, keyFor, resolveEvent } from './routingFlowTopology';
import type { TopoModel } from './routingFlowTopology';
import { getEdgeVisualState, getTrafficIntensity, getTrafficWidth, routingLineStyle } from './routingFlowVisual';

interface Pair { key: string; fromRef: React.RefObject<HTMLDivElement | null>; toRef: React.RefObject<HTMLDivElement | null> }
interface HopEvent { model: string; channel: string; endpoint: string | null; ts: number }
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
      background: 'linear-gradient(90deg, var(--secondary) 40%, var(--border) 50%, var(--secondary) 60%)',
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

    // create circles via DOM (bypasses React render for each frame)
    const glow = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    glow.setAttribute('r', '11');
    glow.setAttribute('fill', 'var(--chart-1)');
    glow.setAttribute('opacity', '0.18');
    svg.appendChild(glow);
    const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    circle.setAttribute('r', '4.5');
    circle.setAttribute('fill', 'var(--chart-1)');
    svg.appendChild(circle);

    const start = performance.now();
    const duration = 720;
    let raf = 0;

    function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const pt = pathEl!.getPointAtLength(t * len);
      circle.setAttribute('cx', String(pt.x));
      circle.setAttribute('cy', String(pt.y));
      circle.setAttribute('opacity', String(1 - t * 0.4));
      glow.setAttribute('cx', String(pt.x));
      glow.setAttribute('cy', String(pt.y));
      glow.setAttribute('opacity', String(0.18 * (1 - t)));
      if (t < 1) raf = requestAnimationFrame(step);
      else { circle.remove(); glow.remove(); doneRef.current(); }
    }
    raf = requestAnimationFrame(step);

    return () => {
      cancelAnimationFrame(raf);
      circle.remove();
      glow.remove();
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

type EndpointTag = { text: string; tone?: 'ok' | 'warn' | 'fail' | 'default'; title?: string; icon?: ReactNode };

const TAG_TONE: Record<'ok' | 'warn' | 'fail' | 'default', { border: string; bg: string; text: string }> = {
  ok: { border: 'var(--chart-2)', bg: 'color-mix(in oklab, var(--chart-2) 12%, transparent)', text: 'var(--chart-2)' },
  warn: { border: 'var(--sidebar-primary)', bg: 'color-mix(in oklab, var(--sidebar-primary) 14%, transparent)', text: 'var(--sidebar-primary)' },
  fail: { border: 'var(--destructive)', bg: 'color-mix(in oklab, var(--destructive) 12%, transparent)', text: 'var(--destructive)' },
  default: { border: 'var(--border)', bg: 'var(--muted)', text: 'var(--muted-foreground)' },
};

/** Show large latencies in seconds like the mock (`P95 54.3s`). */
function formatLatency(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`;
}

// ── FlowNode ────────────────────────────────────────────────────────
function FlowNode({
  nodeRef, title, subtitle, count, heat, skeleton,
  pinged, showBar = true, barPct, tags, onClick, selected = false, pulseKey,
}: {
  nodeRef?: React.RefObject<HTMLDivElement | null>; title: string; subtitle?: string;
  count: number; heat?: 'low' | 'mid' | 'high' | null; skeleton?: boolean;
  pinged?: boolean; showBar?: boolean; barPct?: number; tags?: EndpointTag[];
  onClick?: () => void; selected?: boolean; pulseKey?: string | null;
}) {
  const color = heat ? HEAT_COLOR[heat] : null;
  const width = barPct != null ? barPct : heat === 'high' ? 100 : heat === 'mid' ? 60 : 25;

  return (
    <div ref={nodeRef} role="button" tabIndex={onClick ? 0 : undefined} onClick={onClick} onKeyDown={onClick ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onClick(); } } : undefined} aria-pressed={selected} style={{
      borderRadius: 8,
      border: `2px solid ${selected ? 'var(--chart-1)' : (color || C.border)}`,
      background: selected
        ? 'color-mix(in oklab, var(--card) 90%, var(--chart-1))'
        : C.nodeBg,
      padding: '9px 12px', fontSize: 12.5,
      transition: 'transform 150ms, border-color 300ms, box-shadow 300ms, background-color 300ms',
      transform: pinged ? 'scale(1.03)' : 'scale(1)',
      cursor: onClick ? 'pointer' : 'default',
      boxShadow: selected
        ? '0 0 0 3px color-mix(in oklab, var(--chart-1) 20%, transparent), 0 0 14px color-mix(in oklab, var(--chart-1) 16%, transparent)'
        : 'none',
      animation: pulseKey ? `rfp-node-pulse 380ms cubic-bezier(.2,.8,.2,1)` : undefined,
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
        <span style={{ fontWeight: 600, color: color || C.textPrimary }}>{title}</span>
        {skeleton
        ? <div style={{ width: 32, height: 14, borderRadius: 3, background: 'var(--secondary)', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
        : <AnimatedNumber value={count} style={{ fontSize: 12, color: C.textSecondary }} />}
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: C.textMuted, marginTop: 2 }}>{subtitle}</div>}
      {tags && tags.length > 0 && (
        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 8 }}>
          {tags.map((tag, index) => {
            const tone = TAG_TONE[tag.tone ?? 'default'];
            return (
              <span
                key={index}
                title={tag.title}
                style={{
                  display: 'inline-flex', alignItems: 'center', gap: 3,
                  fontSize: 9.5, fontWeight: 500, padding: '2px 6px', borderRadius: 999,
                  border: `1px solid ${tone.border}`, background: tone.bg, color: tone.text,
                  whiteSpace: 'nowrap',
                }}
              >
                {tag.icon}
                {tag.text}
              </span>
            );
          })}
        </div>
      )}
      {showBar && !skeleton && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: C.barTrack, overflow: 'hidden' }}>
          <div style={{
            height: '100%', borderRadius: 2, width: `${heat ? width : 0}%`,
            background: color || 'transparent',
            transition: 'width 400ms ease, background-color 400ms ease',
          }} />
        </div>
      )}
      {showBar && skeleton && <SkeletonBar />}
    </div>
  );
}

// ── Skeleton panel (loading state) ──────────────────────────────────
function SkeletonPanel() {
  return (
    <div style={{ marginBottom: 16, borderRadius: 10, border: `1px solid ${C.border}`, background: C.cardBg, padding: '20px 24px' }}>
      <div style={{ height: 18, width: 180, borderRadius: 4, background: 'var(--secondary)', marginBottom: 20, animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
      <div style={{ display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', gap: 24, minHeight: 60 }}>
        <div style={{ borderRadius: 8, border: `1.5px solid ${C.border}`, background: C.nodeBg, padding: '9px 12px' }}>
          <div style={{ height: 14, borderRadius: 3, background: 'var(--secondary)', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
          <SkeletonBar />
        </div>
        <div /><div />
        <div />
        <div style={{ borderRadius: 8, border: `1.5px solid ${C.border}`, background: C.nodeBg, padding: '9px 12px' }}>
          <div style={{ height: 14, borderRadius: 3, background: 'var(--secondary)', animation: 'sk-shimmer 1.4s infinite linear', backgroundSize: '200% 100%' }} />
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
  const [liveEventCount, setLiveEventCount] = useState(0);
  const [lastLiveEventAt, setLastLiveEventAt] = useState<number | null>(null);
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
        let ev: { type?: string; model?: string; channel_id?: string; endpoint_id?: number | null; latency_ms?: number };
        try { ev = JSON.parse(e.data); } catch { return; }
        if (!ev || ev.type !== 'route_decided' || typeof ev.model !== 'string' || typeof ev.channel_id !== 'string') return;
        const resolved = resolveEvent(topoRef.current, { model: ev.model, channel_id: ev.channel_id, endpoint_id: ev.endpoint_id });
        if (!resolved) return;
        const { modelName, channelId, endpointKey } = resolved;

        // Only real route_decided events move counts and pulse. RequestCompleted
        // (and any future event) is deliberately silent here — the OTLP trace
        // covers the completed request retroactively.
        setLiveEventCount((c) => c + 1);
        setLastLiveEventAt(Date.now());
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

  return { counts, totalCount, lastEvent, connected, reconnectIn, liveEventCount, lastLiveEventAt };
}

// ── page ────────────────────────────────────────────────────────────
/** Optional scheduling metadata for an endpoint card, keyed by binding
 *  (`<channelId>:<topology endpoint key>`). Sourced from the existing
 *  scheduler policy/topology API by the host page — never fabricated here. */
export type RoutingFlowNodeSelection = {
  model: string;
  channelId?: string;
  endpointKey?: string;
  endpointId?: number | null;
};

export type RoutingFlowEndpointMeta = {
  weight?: number;
  timeoutSecs?: number | null;
  maxTokens?: number | null;
  p95Ms?: number | null;
  routeEligible?: boolean;
  breakerState?: string;
};

/** Optional edge highlight hook, keyed by connector pair key
 *  (e.g. `c2e:gpt-4>ch-1>id:2` for channel→endpoint, `m2c:gpt-4>ch-1`).
 *  'selected' = the edge the scheduler just picked; 'retry' = retry/failover.
 *  Phase 1 exposes the hooks; nothing feeds them until Phase 2D lands
 *  attempt-level data. */
export type RoutingFlowEdgeHighlight = Partial<Record<string, 'selected' | 'retry'>>;

type RoutingFlowProps = {
  embedded?: boolean;
  modelName?: string;
  edgeHighlights?: RoutingFlowEdgeHighlight;
  endpointMeta?: Record<string, RoutingFlowEndpointMeta>;
  onSelectNode?: (node: RoutingFlowNodeSelection | null) => void;
  selectedNode?: RoutingFlowNodeSelection | null;
};

function sameNode(a: RoutingFlowNodeSelection | null | undefined, b: RoutingFlowNodeSelection): boolean {
  return a?.model === b.model
    && a.channelId === b.channelId
    && a.endpointKey === b.endpointKey
    && a.endpointId === b.endpointId;
}

export default function RoutingFlow({ embedded = false, modelName, edgeHighlights = {}, endpointMeta = {}, onSelectNode, selectedNode }: RoutingFlowProps) {
  const [internalSelectedNode, setInternalSelectedNode] = useState<RoutingFlowNodeSelection | null>(null);
  const [selectionPulse, setSelectionPulse] = useState<string | null>(null);
  const activeSelectedNode = selectedNode === undefined ? internalSelectedNode : selectedNode;
  const selectNode = useCallback((node: RoutingFlowNodeSelection) => {
    if (sameNode(activeSelectedNode, node)) {
      setInternalSelectedNode(null);
      onSelectNode?.(null);
      setSelectionPulse(null);
      return;
    }
    setInternalSelectedNode(node);
    setSelectionPulse(`${node.model}:${node.channelId ?? ''}:${node.endpointKey ?? ''}`);
    window.setTimeout(() => setSelectionPulse(null), 380);
    onSelectNode?.(node);
  }, [activeSelectedNode, onSelectNode]);
  const { t } = useTranslation();
  const { data: models, isLoading: mLoading } = usePublicModels();
  const { data: channels, isLoading: cLoading } = useChannels();

  const topology = useMemo(() => {
    if (!models || !channels) return [];
    const all = buildTopology(models, channels).filter((m) => m.channels.length > 0);
    return modelName ? all.filter((m) => m.model === modelName) : all;
  }, [channels, modelName, models]);

  const { counts, totalCount, lastEvent, connected, reconnectIn, liveEventCount, lastLiveEventAt } = useRoutingStream(topology);
  const loading = mLoading || cLoading;

  // ── Refs for all nodes ──
  const containerRef = useRef<HTMLDivElement>(null);
  const modelRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  const channelRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});
  const endpointRefs = useRef<Record<string, React.RefObject<HTMLDivElement | null>>>({});

  // ── Connector pairs (also initialises refs) ──
  const connectorPairs = useMemo(() => {
    const pairs: Pair[] = [];
    topology.forEach((m) => {
      if (!modelRefs.current[m.model]) modelRefs.current[m.model] = { current: null };
      m.channels.forEach((c) => {
        const ck = `${m.model}:${c.id}`;
        if (!channelRefs.current[ck]) channelRefs.current[ck] = { current: null };
        pairs.push({ key: `m2c:${m.model}>${c.id}`, fromRef: modelRefs.current[m.model]!, toRef: channelRefs.current[ck]! });
        c.endpoints.forEach((e) => {
          const ek = `${m.model}:${c.id}:${e.key}`;
          if (!endpointRefs.current[ek]) endpointRefs.current[ek] = { current: null };
          pairs.push({ key: `c2e:${m.model}>${c.id}>${e.key}`, fromRef: channelRefs.current[ck]!, toRef: endpointRefs.current[ek]! });
        });
      });
    });
    return pairs;
  }, [topology]);

  const { svgRef, paths } = useConnectors(containerRef, connectorPairs);

  // Routing eligibility is backend topology truth passed by the selected-model
  // host (endpointMeta.routeEligible). The global endpoints-live API is
  // endpoint_id-only and is intentionally not used as binding-level health.
  // Missing binding data defaults to eligible — never guess down.
  const channelEligible = useMemo(() => {
    const map = new Map<string, boolean>();
    for (const m of topology) {
      for (const c of m.channels) {
        const anyRoutable = c.endpoints.some((e) => {
          if (e.matchId == null) return true;
          return endpointMeta[bindingKey(c.id, e.matchId)]?.routeEligible !== false;
        });
        map.set(`${m.model}>${c.id}`, anyRoutable);
      }
    }
    return map;
  }, [endpointMeta, topology]);
  const maxTraffic = useMemo(() => Math.max(1, ...Object.values(counts)), [counts]);
  const pathWidthMap = useMemo(() => Object.fromEntries(paths.map((p) => {
    const parts = p.key.slice(p.key.indexOf(':') + 1).split('>');
    const m = parts[0];
    const c = parts[1];
    const endpoint = parts[2];
    const count = endpoint
      ? counts[keyFor(m, c, endpoint)] || 0
      : counts[keyFor(m, c)] || 0;
    return [p.key, getTrafficWidth(count, maxTraffic)];
  })), [counts, maxTraffic, paths]);

  // ── Pulse / ping state ──
  const [pulses, setPulses] = useState<{ id: string; pathD: string }[]>([]);
  const [pinged, setPinged] = useState<Record<string, boolean>>({});
  const prevTsRef = useRef(0);
  // Records when a route_decided last traversed each edge (path key → ms).
  // A live edge must stay bright even when the user has selected another node
  // (selection dims the rest; a real-time pulse must still pop through).
  const liveEdgeRef = useRef<Record<string, number>>({});
  const [, forceLiveEdgeRender] = useState(0);

  useEffect(() => {
    if (!lastEvent) return;
    if (lastEvent.ts === prevTsRef.current) return;
    prevTsRef.current = lastEvent.ts;
    const { model, channel, endpoint } = lastEvent;

    if (endpoint) {
      // Hop 2: channel → endpoint
      const epPath = paths.find((p) => p.key === `c2e:${model}>${channel}>${endpoint}`);
      if (epPath) setPulses((prev) => [...prev, { id: `${lastEvent.ts}-ep`, pathD: epPath.d }]);
      liveEdgeRef.current[`c2e:${model}>${channel}>${endpoint}`] = performance.now();
      const ck = `c:${model}:${channel}`;
      const ek = `e:${model}:${channel}:${endpoint}`;
      setPinged((prev) => ({ ...prev, [ck]: true }));
      setTimeout(() => setPinged((prev) => ({ ...prev, [ck]: false })), 300);
      setTimeout(() => {
        setPinged((prev) => ({ ...prev, [ek]: true }));
        setTimeout(() => setPinged((prev) => ({ ...prev, [ek]: false })), 300);
      }, 400);
    } else {
      // Hop 1: model → channel
      const chPath = paths.find((p) => p.key === `m2c:${model}>${channel}`);
      if (chPath) setPulses((prev) => [...prev, { id: `${lastEvent.ts}-ch`, pathD: chPath.d }]);
      liveEdgeRef.current[`m2c:${model}>${channel}`] = performance.now();
      const mk = `m:${model}`;
      const ck = `c:${model}:${channel}`;
      setPinged((prev) => ({ ...prev, [mk]: true }));
      setTimeout(() => setPinged((prev) => ({ ...prev, [mk]: false })), 300);
      setTimeout(() => {
        setPinged((prev) => ({ ...prev, [ck]: true }));
        setTimeout(() => setPinged((prev) => ({ ...prev, [ck]: false })), 300);
      }, 400);
    }
    forceLiveEdgeRender((n) => n + 1);
  }, [lastEvent, paths]);

  const removePulse = useCallback((id: string) => {
    setPulses((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const colLabelStyle: React.CSSProperties = {
    fontSize: 10.5, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.04em',
  };

  return (
    <div style={{ fontFamily: FONT_FAMILY, color: C.textPrimary }}>
      {!embedded ? (
        <>
          <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>{t('routingFlow.title')}</h1>
          <p style={{ fontSize: 13, color: C.textSecondary, margin: '0 0 20px' }}>
            {t('routingFlow.subtitle')}<span> {t('routingFlow.trafficIntensity')}</span>
            <span style={{ color: C.low }}> {t('routingFlow.legendLow')}</span> ·
            <span style={{ color: C.mid }}> {t('routingFlow.legendMid')}</span> ·
            <span style={{ color: C.high }}> {t('routingFlow.legendHigh')}</span>
          </p>

          <div style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: connected ? C.green : C.high }}>
              <span style={{ width: 7, height: 7, borderRadius: '50%', background: connected ? C.green : C.high, animation: connected ? 'rfl-pulse 1.6s infinite' : 'none' }} />
              {connected ? 'LIVE' : reconnectIn > 0 ? `⏳ ${reconnectIn}s` : t('routingFlow.connecting')}
            </div>
            <div style={{ fontSize: 11, color: C.textSecondary }}>
              {t('routingFlow.liveEvents')} <b style={{ color: C.textPrimary }}>{liveEventCount}</b>
              {lastLiveEventAt ? ` · ${new Date(lastLiveEventAt).toLocaleTimeString()}` : ''}
            </div>
            <div style={{ marginLeft: 'auto', display: 'flex', gap: 16, fontSize: 11.5, color: C.textSecondary }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.low, display: 'inline-block' }} /> {t('routingFlow.loadLow')}</div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.mid, display: 'inline-block' }} /> {t('routingFlow.loadMid')}</div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: C.high, display: 'inline-block' }} /> {t('routingFlow.loadHigh')}</div>
            </div>
          </div>
        </>
      ) : null}

      {/* Main content */}
      <div style={{ position: 'relative' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 10, fontSize: 11, color: C.textSecondary }}>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontWeight: 600, color: connected ? C.green : C.high }}>
            <i aria-hidden="true" style={{ width: 7, height: 7, borderRadius: '50%', background: connected ? C.green : C.high }} />
            {connected ? t('routingFlow.liveConnected') : reconnectIn > 0 ? `${t('routingFlow.liveReconnecting')} ${reconnectIn}s` : t('routingFlow.liveDisconnected')}
          </span>
          <span>{t('routingFlow.liveEvents')} <b style={{ color: C.textPrimary }}>{liveEventCount}</b>{lastLiveEventAt ? ` · ${new Date(lastLiveEventAt).toLocaleTimeString()}` : ''}</span>
          <span style={{ marginLeft: 'auto' }}>{t('routingFlow.totalRequests')} <b style={{ color: C.textPrimary }}><AnimatedNumber value={totalCount} /></b></span>
        </div>
        {loading ? (
          <><SkeletonPanel /><SkeletonPanel /></>
        ) : topology.length === 0 ? (
          <div style={{ borderRadius: 10, border: `1px dashed ${C.border}`, background: C.cardBg, padding: '40px 24px', textAlign: 'center', fontSize: 13, color: C.textSecondary }}>
            {t('routingFlow.empty')}
          </div>
        ) : (
          <div style={{ borderRadius: 10, border: `1px solid ${C.border}`, background: C.cardBg, padding: '20px 24px' }}>
            {/* Column header */}
            <div style={{ ...colLabelStyle, display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', marginBottom: 14, alignItems: 'center' }}>
              <div>{t('routingFlow.colModel')}</div>
              <div />
              <div style={{ textAlign: 'center' }}>{t('routingFlow.colChannel')}</div>
              <div />
              <div style={{ textAlign: 'right' }}>{t('routingFlow.colEndpoint')}</div>
            </div>

            {/* Grid with connectors */}
            <div ref={containerRef} style={{ position: 'relative', display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', alignItems: 'start', minHeight: 60 }}>
              <svg ref={svgRef} style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', overflow: 'visible', pointerEvents: 'none' }}>
                {paths.map((p) => {
                  const pathParts = p.key.slice(p.key.indexOf(':') + 1).split('>');
                  const model = pathParts[0];
                  const channel = pathParts[1];
                  const endpoint = pathParts[2];
                  const count = endpoint ? counts[keyFor(model, channel, endpoint)] || 0 : counts[keyFor(model, channel)] || 0;
                  const routeEligible = endpoint
                    ? endpointMeta[bindingKey(channel, Number(endpoint.slice(3)))]?.routeEligible !== false
                    : channelEligible.get(`${model}>${channel}`) !== false;
                  const hl = edgeHighlights[p.key];
                  const selectedPath = activeSelectedNode != null && (
                    model === activeSelectedNode.model
                    && (activeSelectedNode.channelId == null || channel === activeSelectedNode.channelId)
                    && (endpoint == null || activeSelectedNode.endpointKey == null || endpoint === activeSelectedNode.endpointKey)
                  );
                  const state = getEdgeVisualState({ count, routeEligible, selected: hl === 'selected' || selectedPath, retry: hl === 'retry' });
                  const visual = routingLineStyle(state);
                  const dimmed = activeSelectedNode != null && !selectedPath && hl !== 'selected' && hl !== 'retry';
                  const liveRecently = (performance.now() - (liveEdgeRef.current[p.key] ?? 0)) < 900;
                  const effectiveOpacity = liveRecently ? Math.max(visual.opacity, 0.95) : (dimmed ? visual.opacity * 0.22 : visual.opacity);
                  return <path key={p.key} d={p.d} fill="none" stroke={visual.stroke} strokeOpacity={effectiveOpacity} strokeDasharray={visual.dasharray} strokeWidth={liveRecently ? Math.max(pathWidthMap[p.key] ?? 1, 2.5) : (pathWidthMap[p.key] ?? 1)} strokeLinecap="round" style={{ transition: 'stroke-width 600ms ease-out, stroke 300ms, opacity 300ms' }} />;
                })}
                {pulses.map((pulse) => <CometPulse key={pulse.id} pathD={pulse.pathD} onDone={() => removePulse(pulse.id)} />)}
              </svg>

              {/* Col 1: Models */}
              <div style={{ zIndex: 1, gridColumn: 1, display: 'flex', flexDirection: 'column', gap: 10 }}>
                {topology.map((m) => {
                  const cnt = counts[keyFor(m.model)] || 0;
                  return (
                    <div key={m.model} ref={modelRefs.current[m.model]!}>
                      <FlowNode
                        title={m.model}
                        subtitle={m.pattern}
                        count={cnt}
                        pinged={pinged[`m:${m.model}`]}
                        showBar={false}
                        selected={activeSelectedNode?.model === m.model && activeSelectedNode.channelId == null}
                        pulseKey={selectionPulse === `${m.model}::` ? selectionPulse : null}
                        onClick={() => selectNode({ model: m.model })}
                      />
                    </div>
                  );
                })}
              </div>

              {/* Col 3: Channels */}
              <div style={{ zIndex: 1, gridColumn: 3, display: 'flex', flexDirection: 'column', gap: 10 }}>
                {topology.flatMap((m) => {
                  const siblingCounts = m.channels.map((c2) => counts[keyFor(m.model, c2.id)] || 0);
                  return m.channels.map((c) => {
                    const cnt = counts[keyFor(m.model, c.id)] || 0;
                    const ck = `${m.model}:${c.id}`;
                    const cls = getTrafficIntensity(cnt, siblingCounts);
                    return (
                      <div key={ck} ref={channelRefs.current[ck]!}>
                        <FlowNode
                          title={c.name}
                          count={cnt}
                          heat={cls}
                          pinged={pinged[`c:${ck}`]}
                          barPct={Math.round((cnt / (siblingCounts.reduce((a, b) => a + b, 0) || 1)) * 100)}
                          selected={activeSelectedNode?.model === m.model && activeSelectedNode.channelId === c.id && activeSelectedNode.endpointKey == null}
                          pulseKey={selectionPulse === `${m.model}:${c.id}:` ? selectionPulse : null}
                          onClick={() => selectNode({ model: m.model, channelId: c.id })}
                        />
                      </div>
                    );
                  });
                })}
              </div>

              {/* Col 5: Endpoints */}
              <div style={{ zIndex: 1, gridColumn: 5, display: 'flex', flexDirection: 'column', gap: 10 }}>
                {topology.flatMap((m) =>
                  m.channels.flatMap((c) => {
                    const epSiblings = c.endpoints.map((e2) => counts[keyFor(m.model, c.id, e2.key)] || 0);
                    return c.endpoints.map((e) => {
                      const cnt = counts[keyFor(m.model, c.id, e.key)] || 0;
                      const ek = `${m.model}:${c.id}:${e.key}`;
                      const cls = getTrafficIntensity(cnt, epSiblings);
                      const meta = endpointMeta[bindingKey(c.id, e.matchId)];
                      const breakerState = meta?.breakerState?.toLowerCase();
                      const breakerTone = breakerState === 'closed'
                        ? 'ok'
                        : breakerState === 'half_open'
                          ? 'warn'
                          : breakerState === 'open'
                            ? 'fail'
                            : 'default';
                      const BreakerIcon = breakerTone === 'ok'
                        ? ShieldCheck
                        : breakerTone === 'warn'
                          ? ShieldAlert
                          : breakerTone === 'fail'
                            ? ShieldAlert
                            : ShieldBan;
                      const breakerLabel = breakerTone === 'ok'
                        ? t('routingFlow.breakerNormal')
                        : breakerTone === 'warn'
                          ? t('routingFlow.breakerRecovering')
                          : breakerTone === 'fail'
                            ? t('routingFlow.breakerOpen')
                            : t('routingFlow.breakerDisabled');
                      const tags: EndpointTag[] = [];
                      tags.push({
                        text: breakerLabel,
                        tone: breakerTone,
                        title: breakerState ? `${t('routingFlow.endpointBreaker')}: ${breakerState.toUpperCase()}` : undefined,
                        icon: <BreakerIcon size={12} strokeWidth={2.2} aria-hidden="true" />,
                      });
                      if (meta?.routeEligible != null) {
                        tags.push({
                          text: meta.routeEligible ? t('routingFlow.endpointRouteEligible') : t('routingFlow.lineUnavailable'),
                          tone: meta.routeEligible ? 'ok' : 'fail',
                        });
                      }
                      if (meta?.weight != null) tags.push({ text: `${t('routingFlow.endpointWeight')} ${meta.weight}` });
                      if (meta?.timeoutSecs != null) tags.push({ text: `${t('routingFlow.endpointTimeout')} ${meta.timeoutSecs}s` });
                      if (meta?.maxTokens != null) tags.push({ text: `${t('routingFlow.endpointMaxTokens')} ${meta.maxTokens}` });
                      if (meta?.p95Ms != null) {
                        tags.push({ text: `${t('routingFlow.endpointChannelP95Short')} ${formatLatency(meta.p95Ms)}`, title: t('routingFlow.endpointChannelP95') });
                      }
                      return (
                        <div key={ek} ref={endpointRefs.current[ek]!}>
                          <FlowNode
                            title={`${t('routingFlow.endpointLabel')} ${e.label}`}
                            subtitle={`${e.url} · ${c.name}`}
                            count={cnt}
                            heat={cls}
                            tags={tags}
                            pinged={pinged[`e:${ek}`]}
                            showBar={false}
                            selected={activeSelectedNode?.model === m.model && activeSelectedNode.channelId === c.id && activeSelectedNode.endpointKey === e.key}
                            pulseKey={selectionPulse === `${m.model}:${c.id}:${e.key}` ? selectionPulse : null}
                            onClick={() => selectNode({ model: m.model, channelId: c.id, endpointKey: e.key, endpointId: e.matchId })}
                          />
                        </div>
                      );
                    });
                  })
                )}
              </div>
            </div>
            <div style={{ display: 'flex', gap: 14, marginTop: 8, fontSize: 11.5, color: C.textSecondary }}>
              {[
                ['var(--border)', t('routingFlow.lineHealthyZero'), undefined],
                ['var(--muted-foreground)', t('routingFlow.lineUnavailable'), '6 5'],
                ['var(--chart-1)', t('routingFlow.lineActive'), undefined],
              ].map(([color, label, dash]) => (
                <div key={String(label)} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <svg width="24" height="8"><line x1="0" x2="24" y1="4" y2="4" stroke={String(color)} strokeWidth="2" strokeDasharray={dash ? String(dash) : undefined} /></svg>
                  {label}
                </div>
              ))}
            </div>
          </div>
        )}

        {!loading && !connected && topology.length > 0 && (
          <div style={{ position: 'absolute', inset: 0, borderRadius: 10, background: 'color-mix(in oklab, var(--card) 55%, transparent)', backdropFilter: 'blur(1px)', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 10 }}>
            <span style={{ fontSize: 14, color: C.textSecondary, fontWeight: 500 }}>
              🔌 {t('routingFlow.connecting')}... {reconnectIn > 0 ? `(${reconnectIn}s)` : ''}
            </span>
          </div>
        )}
      </div>

      <style>{`
        @keyframes rfl-pulse { 0% { box-shadow: 0 0 0 0 color-mix(in oklab, var(--chart-2) 50%, transparent); } 70% { box-shadow: 0 0 0 6px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
        @keyframes sk-shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }
        @keyframes rfp-node-pulse { 0% { transform: scale(1); } 45% { transform: scale(1.035); } 100% { transform: scale(1); } }
      `}</style>
    </div>
  );
}
