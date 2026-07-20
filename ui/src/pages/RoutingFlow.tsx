import { useState, useRef, useEffect, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useModels } from '@/api/models';
import { useChannels } from '@/api/channels';
import { fetchRoutingFlowSnapshot } from '@/api/routing';
import type { Channel, Model } from '@/types';

/**
 * ============================================================================
 * 实时路由流量面板 — 真实数据版
 * ============================================================================
 * 布局 / 样式 / 动效严格还原设计示例（内联样式），数据源换成真实来源：
 *   拓扑  ← useModels() + useChannels()（模型的渠道绑定 → 渠道的端点）
 *   实时流 ← WebSocket /admin/api/health/ws（同源 httpOnly cookie 认证）
 *           每条 RequestEvent = { model, channel_id, endpoint_id, latency_ms, success }
 * ============================================================================
 */

// ---------------------------------------------------------------------------
// 设计 token（对应示例 :root CSS 变量）
// ---------------------------------------------------------------------------
const C = {
  bg: '#f5f5f3',
  cardBg: '#ffffff',
  border: '#e4e3de',
  line: '#d8d7d1',
  textPrimary: '#1a1a18',
  textSecondary: '#6b6a64',
  textMuted: '#9a988f',
  nodeBg: '#fafaf8',
  barTrack: '#eeede8',
  green: '#1a8a3d',
  low: '#4a7fc9',
  mid: '#d99a2b',
  high: '#c94a4a',
};

const LOAD_COLOR: Record<string, string> = { low: C.low, mid: C.mid, high: C.high };

// ---------------------------------------------------------------------------
// 拓扑类型 — 由真实模型/渠道数据组装而成
// ---------------------------------------------------------------------------
interface TopoEndpoint {
  key: string; // 稳定 key（用于 refs / 计数 / 连线）
  matchId: number | null; // 对应 RequestEvent.endpoint_id
  label: string; // 显示名，如 "端点 1"
  url: string;
}
interface TopoChannel {
  id: string; // 渠道 id（== RequestEvent.channel_id）
  name: string;
  endpoints: TopoEndpoint[];
}
interface TopoModel {
  model: string; // 模型名
  pattern: string; // model_pattern
  channels: TopoChannel[];
}

const keyFor = (...parts: (string | number)[]) => parts.join('>');

// 负载档位判定（基于该节点占同级兄弟节点的比例）
function loadClass(count: number, siblingCounts: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblingCounts);
  const ratio = count / max;
  if (ratio >= 0.66) return 'high';
  if (ratio >= 0.33) return 'mid';
  return 'low';
}

// 复刻后端 match_pattern（src/service/routing.rs），用于把事件 model 归类到拓扑
function matchPattern(text: string, pattern: string): boolean {
  if (pattern === '*') return true;
  if (!pattern.includes('*')) return text === pattern;
  const parts = pattern.split('*');
  if (parts.length === 2) {
    const [prefix, suffix] = parts;
    return (prefix === '' || text.startsWith(prefix)) && (suffix === '' || text.endsWith(suffix));
  }
  if (parts.length === 3) {
    const [prefix, middle, suffix] = parts;
    return text.startsWith(prefix) && text.includes(middle) && text.endsWith(suffix);
  }
  return pattern === text;
}

// 把一条 RequestEvent 解析到拓扑节点
function resolveEvent(
  topology: TopoModel[],
  ev: { model: string; channel_id: string; endpoint_id?: number | null },
): { modelName: string; channelId: string; endpointKey: string | null } | null {
  const m =
    topology.find((t) => t.model === ev.model) ||
    topology.find((t) => matchPattern(ev.model, t.pattern));
  if (!m) return null;
  const ch = m.channels.find((c) => c.id === ev.channel_id);
  if (!ch) return null;
  let ep: TopoEndpoint | undefined;
  if (ev.endpoint_id != null) ep = ch.endpoints.find((e) => e.matchId === ev.endpoint_id);
  if (!ep) ep = ch.endpoints[0]; // 端点不明时归到首个端点，保证端点级动画/计数
  return { modelName: m.model, channelId: ch.id, endpointKey: ep ? ep.key : null };
}

// ---------------------------------------------------------------------------
// 拓扑组装：models（含渠道绑定）× channels（含端点）
// 同名的模型会合并为一个面板（多个模型条目可能绑定不同渠道）。
// ---------------------------------------------------------------------------
function buildTopology(models: Model[], channels: Channel[]): TopoModel[] {
  const channelMap = new Map(channels.map((c) => [c.id, c]));
  const merged = new Map<string, TopoModel>();
  for (const m of models) {
    const key = m.name;
    let entry = merged.get(key);
    if (!entry) {
      entry = { model: m.name, pattern: m.model_pattern, channels: [] };
      merged.set(key, entry);
    }
    for (const mc of m.channels) {
      const ch = channelMap.get(mc.channel_id);
      if (!ch) continue;
      if (entry.channels.some((ec) => ec.id === ch.id)) continue;
      entry.channels.push({
        id: ch.id,
        name: ch.name || ch.id,
        endpoints: ch.endpoints.map((e, i) => ({
          key: e.id != null ? `id:${e.id}` : `${ch.id}#${i}`,
          matchId: e.id ?? null,
          label: `端点 ${i + 1}`,
          url: e.url,
        })),
      });
    }
  }
  return [...merged.values()];
}

// ---------------------------------------------------------------------------
// 数据源 hook — WebSocket 实时请求流
// ---------------------------------------------------------------------------
function useRoutingStream(topology: TopoModel[]) {
  const [totalCount, setTotalCount] = useState(0);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [lastEvent, setLastEvent] = useState<{ model: string; channel: string; endpoint: string | null; ts: number } | null>(null);
  const [connected, setConnected] = useState(false);

  // 用 ref 持有最新拓扑，避免拓扑变化触发 WS 重连
  const topoRef = useRef(topology);
  topoRef.current = topology;

  // Load 24h snapshot on mount as initial counts.
  // Historical records often lack endpoint_id (NULL); this pass spreads
  // channel-level counts across known endpoints so the panel doesn't
  // show zero for endpoints that actually served traffic.
  useEffect(() => {
    fetchRoutingFlowSnapshot().then((snap) => {
      if (Object.keys(snap).length > 0) {
        const patched = { ...snap };
        for (const m of topoRef.current) {
          
          for (const c of m.channels) {
            const ck = keyFor(m.model, c.id);
            const chCount = patched[ck] || 0;
            const epSum = c.endpoints.reduce((s, e) => s + (patched[keyFor(m.model, c.id, e.key)] || 0), 0);
            // If channel has traffic but endpoint-level is missing/null, spread evenly
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
        const total = Object.entries(patched)
          .filter(([k]) => k.split(">").length === 1)
          .reduce((s, [, v]) => s + v, 0);
        setTotalCount(total);
      }
    }).catch(() => {});
  }, []);
  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;

    function connect() {
      const proto = window.location.protocol === 'https:' ? 'wss' : 'ws';
      ws = new WebSocket(`${proto}://${window.location.host}/admin/api/health/ws`);

      ws.onopen = () => setConnected(true);

      ws.onmessage = (e) => {
        let ev: { model?: string; channel_id?: string; endpoint_id?: number | null };
        try {
          ev = JSON.parse(e.data);
        } catch {
          return;
        }
        if (!ev || typeof ev.model !== 'string' || typeof ev.channel_id !== 'string') return;
        const resolved = resolveEvent(topoRef.current, {
          model: ev.model,
          channel_id: ev.channel_id,
          endpoint_id: ev.endpoint_id,
        });
        if (!resolved) return;
        const { modelName, channelId, endpointKey } = resolved;

        setCounts((prev) => {
          const next = { ...prev };
          const mk = keyFor(modelName);
          next[mk] = (next[mk] || 0) + 1;
          const ck = keyFor(modelName, channelId);
          next[ck] = (next[ck] || 0) + 1;
          if (endpointKey) {
            const ek = keyFor(modelName, channelId, endpointKey);
            next[ek] = (next[ek] || 0) + 1;
          }
          return next;
        });
        setTotalCount((c) => c + 1);
        setLastEvent({ model: modelName, channel: channelId, endpoint: endpointKey, ts: performance.now() });
      };

      ws.onclose = () => {
        setConnected(false);
        if (!closed) retry = setTimeout(connect, 2000); // 自动重连
      };
      ws.onerror = () => {
        try {
          ws?.close();
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
        ws?.close();
      } catch {
        /* noop */
      }
    };
  }, []);

  return { counts, totalCount, lastEvent, connected };
}

// ---------------------------------------------------------------------------
// FlowNode — 节点卡片
// ---------------------------------------------------------------------------
function FlowNode({
  nodeRef,
  title,
  subtitle,
  count,
  loadCls,
  pinged,
  showBar = true,
  barPct,
}: {
  nodeRef?: React.RefObject<HTMLDivElement | null>;
  title: string;
  subtitle?: string;
  count: number;
  loadCls?: 'low' | 'mid' | 'high' | null;
  pinged?: boolean;
  showBar?: boolean;
  barPct?: number;
}) {
  const color = loadCls ? LOAD_COLOR[loadCls] : null;
  const width = barPct != null ? barPct : loadCls === 'high' ? 100 : loadCls === 'mid' ? 60 : 25;

  return (
    <div
      ref={nodeRef}
      style={{
        borderRadius: 8,
        border: `1.5px solid ${color || C.border}`,
        background: C.nodeBg,
        padding: '9px 12px',
        fontSize: 12.5,
        transition: 'transform 150ms, border-color 300ms',
        transform: pinged ? 'scale(1.03)' : 'scale(1)',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
        <span style={{ fontWeight: 600, color: color || C.textPrimary }}>{title}</span>
        <span style={{ fontSize: 12, color: C.textSecondary, fontVariantNumeric: 'tabular-nums' }}>{count}</span>
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: C.textMuted, marginTop: 2 }}>{subtitle}</div>}
      {showBar && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: C.barTrack, overflow: 'hidden' }}>
          <div
            style={{
              height: '100%',
              borderRadius: 2,
              width: `${loadCls ? width : 0}%`,
              background: color || 'transparent',
              transition: 'width 400ms ease, background-color 400ms ease',
            }}
          />
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 连线计算 hook（ResizeObserver 跟踪节点位置）
// ---------------------------------------------------------------------------
interface Pair {
  key: string;
  fromRef: React.RefObject<HTMLDivElement | null>;
  toRef: React.RefObject<HTMLDivElement | null>;
}
function useConnectors(containerRef: React.RefObject<HTMLDivElement | null>, pairs: Pair[]) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [paths, setPaths] = useState<{ key: string; d: string }[]>([]);

  const recompute = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const cRect = container.getBoundingClientRect();

    const next = pairs
      .map(({ key, fromRef, toRef }) => {
        const fromEl = fromRef.current;
        const toEl = toRef.current;
        if (!fromEl || !toEl) return null;
        const fr = fromEl.getBoundingClientRect();
        const tr = toEl.getBoundingClientRect();
        const p0 = { x: fr.right - cRect.left, y: fr.top + fr.height / 2 - cRect.top };
        const p1 = { x: tr.left - cRect.left, y: tr.top + tr.height / 2 - cRect.top };
        const midX = (p0.x + p1.x) / 2;
        const d = `M ${p0.x} ${p0.y} C ${midX} ${p0.y}, ${midX} ${p1.y}, ${p1.x} ${p1.y}`;
        return { key, d };
      })
      .filter((v): v is { key: string; d: string } => !!v);

    setPaths(next);
  }, [containerRef, pairs]);

  useEffect(() => {
    recompute();
    const ro = new ResizeObserver(recompute);
    if (containerRef.current) ro.observe(containerRef.current);
    window.addEventListener('resize', recompute);
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', recompute);
    };
  }, [recompute, containerRef]);

  return { svgRef, paths };
}

// 沿路径飞行的脉冲点
function FlowPulse({ pathD, duration = 550, onDone }: { pathD: string; duration?: number; onDone: () => void }) {
  const dotRef = useRef<SVGCircleElement>(null);
  const pathElRef = useRef<SVGPathElement>(null);

  useEffect(() => {
    const pathEl = pathElRef.current;
    if (!pathEl) return;
    const len = pathEl.getTotalLength();
    const start = performance.now();
    let raf: number;

    function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const pt = pathEl!.getPointAtLength(t * len);
      if (dotRef.current) {
        dotRef.current.setAttribute('cx', String(pt.x));
        dotRef.current.setAttribute('cy', String(pt.y));
        dotRef.current.setAttribute('opacity', String(1 - t * 0.3));
      }
      if (t < 1) raf = requestAnimationFrame(step);
      else onDone();
    }
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [duration, onDone]);

  return (
    <>
      <path ref={pathElRef} d={pathD} fill="none" stroke="none" />
      <circle ref={dotRef} r="3.5" fill={C.low} />
    </>
  );
}

// ---------------------------------------------------------------------------
// ModelPanel — 每个模型一个面板
// ---------------------------------------------------------------------------
function ModelPanel({
  model,
  counts,
  lastEvent,
}: {
  model: TopoModel;
  counts: Record<string, number>;
  lastEvent: { model: string; channel: string; endpoint: string | null; ts: number } | null;
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
        pairs.push({
          key: keyFor(model.model, c.id, e.key),
          fromRef: channelNodeRefs.current[c.id],
          toRef: endpointNodeRefs.current[e.key],
        });
      });
    });
    return pairs;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model]);

  const { svgRef, paths } = useConnectors(containerRef, connectorPairs);

  useEffect(() => {
    if (!lastEvent || lastEvent.model !== model.model) return;
    const { channel, endpoint, ts } = lastEvent;

    const chPath = paths.find((p) => p.key === keyFor(model.model, channel));
    const epPath = endpoint ? paths.find((p) => p.key === keyFor(model.model, channel, endpoint)) : undefined;

    if (chPath) setPulses((prev) => [...prev, { id: `${ts}-ch`, pathD: chPath.d }]);
    let epTimer: ReturnType<typeof setTimeout> | undefined;
    if (epPath) {
      epTimer = setTimeout(() => {
        setPulses((prev) => [...prev, { id: `${ts}-ep`, pathD: epPath.d }]);
      }, 200);
    }

    const keysToPing = [keyFor(model.model), keyFor(model.model, channel)];
    if (endpoint) keysToPing.push(keyFor(model.model, channel, endpoint));
    const pingTimers = keysToPing.map((k, i) =>
      setTimeout(() => {
        setPinged((prev) => ({ ...prev, [k]: true }));
        setTimeout(() => setPinged((prev) => ({ ...prev, [k]: false })), 200);
      }, i * 150),
    );

    return () => {
      if (epTimer) clearTimeout(epTimer);
      pingTimers.forEach(clearTimeout);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [lastEvent]);

  const removePulse = useCallback((id: string) => {
    setPulses((prev) => prev.filter((p) => p.id !== id));
  }, []);

  const modelCount = counts[keyFor(model.model)] || 0;
  const channelCounts = model.channels.map((c) => counts[keyFor(model.model, c.id)] || 0);
  const colLabelStyle: React.CSSProperties = {
    fontSize: 10.5,
    color: C.textMuted,
    textTransform: 'uppercase',
    letterSpacing: '0.04em',
  };

  return (
    <div
      style={{
        marginBottom: 16,
        borderRadius: 10,
        border: `1px solid ${C.border}`,
        background: C.cardBg,
        padding: '20px 24px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 18, fontSize: 14, fontWeight: 600 }}>
        <span>{model.model}</span>
        <span
          style={{
            fontSize: 11,
            fontWeight: 400,
            color: C.textMuted,
            background: '#f0efe9',
            padding: '1px 8px',
            borderRadius: 4,
            fontFamily: 'SF Mono, Consolas, monospace',
          }}
        >
          {model.pattern}
        </span>
        <span style={{ marginLeft: 'auto', fontSize: 12, fontWeight: 400, color: C.textSecondary }}>
          {t('routingFlow.reqCountPrefix')} <b style={{ color: C.textPrimary, fontWeight: 600 }}>{modelCount}</b>{' '}
          {t('routingFlow.reqCountSuffix')}
        </span>
      </div>

      <div
        ref={containerRef}
        style={{
          position: 'relative',
          display: 'grid',
          gridTemplateColumns: '200px 1fr 200px 1fr 200px',
          alignItems: 'center',
          minHeight: 60,
        }}
      >
        <svg
          ref={svgRef}
          style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', overflow: 'visible', pointerEvents: 'none' }}
        >
          {paths.map((p) => (
            <path key={p.key} d={p.d} fill="none" stroke={C.line} strokeWidth="1.5" />
          ))}
          {pulses.map((pulse) => (
            <FlowPulse key={pulse.id} pathD={pulse.pathD} onDone={() => removePulse(pulse.id)} />
          ))}
        </svg>

        {/* 列 1：模型 */}
        <div style={{ zIndex: 1, gridColumn: 1, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colModel')}</div>
          <FlowNode nodeRef={modelNodeRef} title={model.model} count={modelCount} pinged={pinged[keyFor(model.model)]} showBar={false} />
        </div>
        <div />

        {/* 列 2：路由渠道（负载均衡） */}
        <div style={{ zIndex: 1, gridColumn: 3, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colChannel')}</div>
          {model.channels.map((c) => {
            const cnt = counts[keyFor(model.model, c.id)] || 0;
            const cls = loadClass(cnt, channelCounts);
            const max = Math.max(1, ...channelCounts);
            return (
              <FlowNode
                key={c.id}
                nodeRef={channelNodeRefs.current[c.id]}
                title={c.name}
                count={cnt}
                loadCls={cls}
                barPct={Math.round((cnt / max) * 100)}
                pinged={pinged[keyFor(model.model, c.id)]}
              />
            );
          })}
        </div>
        <div />

        {/* 列 3：渠道端点（负载均衡） */}
        <div style={{ zIndex: 1, gridColumn: 5, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div style={colLabelStyle}>{t('routingFlow.colEndpoint')}</div>
          {model.channels.flatMap((c) => {
            const epCounts = c.endpoints.map((e) => counts[keyFor(model.model, c.id, e.key)] || 0);
            const emax = Math.max(1, ...epCounts);
            return c.endpoints.map((e) => {
              const cnt = counts[keyFor(model.model, c.id, e.key)] || 0;
              const cls = loadClass(cnt, epCounts);
              return (
                <FlowNode
                  key={e.key}
                  nodeRef={endpointNodeRefs.current[e.key]}
                  title={e.label}
                  subtitle={`${e.url} · ${c.name}`}
                  count={cnt}
                  loadCls={cls}
                  barPct={Math.round((cnt / emax) * 100)}
                  pinged={pinged[keyFor(model.model, c.id, e.key)]}
                />
              );
            });
          })}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 顶层页面组件
// ---------------------------------------------------------------------------
export default function RoutingFlow() {
  const { t } = useTranslation();
  const { data: models, isLoading: mLoading } = useModels();
  const { data: channels, isLoading: cLoading } = useChannels();

  const topology = useMemo(() => {
    if (!models || !channels) return [];
    // 只保留至少绑定了一个渠道的模型
    return buildTopology(models, channels).filter((m) => m.channels.length > 0);
  }, [models, channels]);

  const { counts, totalCount, lastEvent, connected } = useRoutingStream(topology);

  const loading = mLoading || cLoading;
  const fontFamily = '-apple-system, PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

  return (
    <div style={{ fontFamily, color: C.textPrimary }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>{t('routingFlow.title')}</h1>
      <p style={{ fontSize: 13, color: C.textSecondary, margin: '0 0 20px' }}>
        {t('routingFlow.subtitle')}
        <span style={{ color: C.low }}> {t('routingFlow.legendLow')}</span> ·
        <span style={{ color: C.mid }}> {t('routingFlow.legendMid')}</span> ·
        <span style={{ color: C.high }}> {t('routingFlow.legendHigh')}</span>
      </p>

      <div style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: connected ? C.green : C.textMuted }}>
          <span
            style={{
              width: 7,
              height: 7,
              borderRadius: '50%',
              background: connected ? C.green : C.textMuted,
              animation: connected ? 'gw-pulse 1.6s infinite' : 'none',
            }}
          />
          {connected ? 'LIVE' : t('routingFlow.connecting')}
        </div>
        <div style={{ fontSize: 12, color: C.textSecondary }}>
          {t('routingFlow.totalRequests')}{' '}
          <b style={{ fontSize: 15, color: C.textPrimary, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>
            {totalCount.toLocaleString()}
          </b>
        </div>
        <div style={{ marginLeft: 'auto', display: 'flex', gap: 16, fontSize: 11.5, color: C.textSecondary }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ width: 22, height: 6, borderRadius: 3, background: C.low, display: 'inline-block' }} /> {t('routingFlow.loadLow')}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ width: 22, height: 6, borderRadius: 3, background: C.mid, display: 'inline-block' }} /> {t('routingFlow.loadMid')}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ width: 22, height: 6, borderRadius: 3, background: C.high, display: 'inline-block' }} /> {t('routingFlow.loadHigh')}
          </div>
        </div>
      </div>

      {loading ? (
        <div style={{ fontSize: 13, color: C.textSecondary }}>{t('common.loading')}</div>
      ) : topology.length === 0 ? (
        <div
          style={{
            borderRadius: 10,
            border: `1px dashed ${C.border}`,
            background: C.cardBg,
            padding: '40px 24px',
            textAlign: 'center',
            fontSize: 13,
            color: C.textSecondary,
          }}
        >
          {t('routingFlow.empty')}
        </div>
      ) : (
        topology.map((m) => <ModelPanel key={m.model} model={m} counts={counts} lastEvent={lastEvent} />)
      )}

      <style>{`
        @keyframes gw-pulse {
          0% { box-shadow: 0 0 0 0 rgba(26,138,61,0.5); }
          70% { box-shadow: 0 0 0 6px rgba(26,138,61,0); }
          100% { box-shadow: 0 0 0 0 rgba(26,138,61,0); }
        }
      `}</style>
    </div>
  );
}
