import { useCallback, useEffect, useRef, useState } from 'react';
import { useRoutingHealth } from '@/api/health';
import { Card } from '@/components/ui/card';

/* ── Helpers ── */
function keyOf(...parts: (string | number)[]) { return parts.join('>'); }

function loadClass(count: number, siblings: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblings);
  const r = count / max;
  if (r >= 0.66) return 'high';
  if (r >= 0.33) return 'mid';
  return 'low';
}

const LOAD_COLORS: Record<string, string> = { low: '#4a7fc9', mid: '#d99a2b', high: '#c94a4a' };

/* ── Data topology built from real models ── */
interface TopoEndpoint { id: string; url: string; weight: number; }
interface TopoChannel { id: string; weight: number; endpoints: TopoEndpoint[]; }
interface TopoModel { model: string; pattern: string; channels: TopoChannel[]; }

/* ── Component ── */
export default function HealthPage() {
  const { data } = useRoutingHealth();
  const summary = data?.summary;
  const [counts, setCounts] = useState<Record<string, number>>({});
  const totalRef = useRef(0);
  const [total, setTotal] = useState(0);
  const wsRef = useRef<WebSocket | null>(null);
  const [, forceUpdate] = useState(0);

  // Build topology from real data
  const topology: TopoModel[] = (data?.models ?? []).map((m: any) => ({
    model: m.name,
    pattern: m.model_pattern,
    channels: m.channels.map((ch: any) => ({
      id: ch.channel_name || ch.channel_id,
      weight: Math.max(1, ch.requests || 1),
      endpoints: (ch.endpoints || []).map((ep: any) => ({
        id: `端点 ${ep.endpoint_id}`,
        url: ep.url || '',
        weight: Math.max(1, ep.available ? 1 : 0.1),
      })),
      ...(ch.endpoints?.length ? {} : { endpoints: [{ id: '端点 1', url: '', weight: 1 }] }),
    })),
  }));

  // WebSocket: connect and receive real-time request events
  const connectWs = useCallback(() => {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${proto}//${window.location.host}/admin/api/health/ws`;
    const ws = new WebSocket(wsUrl);

    ws.onmessage = (event) => {
      try {
        const ev = JSON.parse(event.data);
        if (!ev.model || !ev.channel_id) return;
        const mk = keyOf('m', ev.model);
        const ck = keyOf('c', ev.model, ev.channel_id);
        const ek = ev.endpoint_id ? keyOf('e', ev.model, ev.channel_id, ev.endpoint_id) : undefined;

        setCounts((prev) => ({
          ...prev,
          [mk]: (prev[mk] || 0) + 1,
          [ck]: (prev[ck] || 0) + 1,
          ...(ek ? { [ek]: (prev[ek] || 0) + 1 } : {}),
        }));
        totalRef.current++;
        setTotal(totalRef.current);
        forceUpdate((u) => u + 1);
      } catch { /* ignore parse errors */ }
    };

    ws.onclose = () => {
      wsRef.current = null;
      // Auto-reconnect after 3 seconds
      setTimeout(connectWs, 3000);
    };

    ws.onerror = () => { ws.close(); };
    wsRef.current = ws;
  }, []);

  useEffect(() => {
    connectWs();
    return () => { wsRef.current?.close(); };
  }, [connectWs]);

  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

  return (
    <div style={{ fontFamily: '-apple-system,"PingFang SC","Microsoft YaHei",Segoe UI,sans-serif', color: '#1a1a18' }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>实时路由流量面板</h1>
      <p style={{ fontSize: 13, color: '#6b6a64', margin: '0 0 20px' }}>
        模型&nbsp;→&nbsp;路由渠道（负载均衡）→&nbsp;渠道端点（负载均衡），颜色表示相对负载：
        <span style={{ color: '#4a7fc9' }}> 蓝=低</span>
        <span style={{ color: '#d99a2b' }}> · 黄=中</span>
        <span style={{ color: '#c94a4a' }}> · 红=高</span>
      </p>

      {/* Top bar */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: '#1a8a3d' }}>
          <span style={{ width: 7, height: 7, borderRadius: '50%', background: '#1a8a3d', boxShadow: '0 0 0 0 rgba(26,138,61,0.5)', animation: 'pulse-dot 1.6s infinite' }} />
          LIVE
        </div>
        <div style={{ fontSize: 12, color: '#6b6a64' }}>
          总请求数 <b style={{ fontSize: 15, color: '#1a1a18', fontVariantNumeric: 'tabular-nums' }}>{total.toLocaleString()}</b>
        </div>
        <div style={{ display: 'flex', gap: 16, marginLeft: 'auto', fontSize: 11.5, color: '#6b6a64' }}>
          {[{ c: '#4a7fc9', l: '低负载' }, { c: '#d99a2b', l: '中负载' }, { c: '#c94a4a', l: '高负载' }].map((x) => (
            <span key={x.l} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
              <span style={{ width: 22, height: 6, borderRadius: 3, background: x.c }} />
              {x.l}
            </span>
          ))}
        </div>
      </div>

      {/* Summary cards */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 20 }}>
        {[['总请求数 / 24h', summary?.total_requests_24h?.toLocaleString() ?? '-'],
          ['整体成功率', summary ? pct(summary.overall_success_rate) : '-'],
          ['活跃渠道数', `${summary?.active_channels ?? '-'}`],
          ['熔断中渠道', `${summary?.broken_channels ?? '-'}`],
        ].map(([label, val]) => (
          <Card key={label as string} style={{ padding: '14px 16px' }}>
            <div style={{ fontSize: 12, color: '#6b6a64', marginBottom: 6 }}>{label}</div>
            <div style={{ fontSize: 22, fontWeight: 600 }}>{val as string}</div>
          </Card>
        ))}
      </div>

      {/* Panels */}
      {topology.map((m) => (
        <ModelPanel key={m.model} m={m} counts={counts} />
      ))}
    </div>
  );
}

function pickWeighted<T extends { weight: number }>(items: T[]): T {
  const total = items.reduce((s, i) => s + i.weight, 0);
  let r = Math.random() * total;
  for (const it of items) { if (r < it.weight) return it; r -= it.weight; }
  return items[items.length - 1];
}

function ModelPanel({ m, counts }: { m: TopoModel; counts: Record<string, number> }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const pathRefs = useRef<Map<string, SVGPathElement>>(new Map());
  const nodeRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const pulseId = useRef(0);

  // Increment counts and trigger animation
  const mk = keyOf('m', m.model);
  const chData = m.channels.map((c) => ({
    key: keyOf('c', m.model, c.id),
    label: c.id,
    count: counts[keyOf('c', m.model, c.id)] || 0,
    endpoints: c.endpoints.map((e) => ({
      key: keyOf('e', m.model, c.id, e.id),
      label: e.id,
      url: e.url,
      count: counts[keyOf('e', m.model, c.id, e.id)] || 0,
    })),
  }));
  const chCounts = chData.map((c) => c.count);
  const modelTotal = counts[mk] || 0;

  // SVG redraw
  useEffect(() => {
    if (!containerRef.current) return;
    const box = containerRef.current.getBoundingClientRect();

    function center(el: HTMLElement, side: 'l' | 'r') {
      const r = el.getBoundingClientRect();
      return { x: side === 'r' ? r.right - box.left : r.left - box.left, y: r.top + r.height / 2 - box.top };
    }

    const modelEl = nodeRefs.current.get(mk);
    if (!modelEl) return;
    const p0 = center(modelEl, 'r');

    const paths: { key: string; d: string }[] = [];
    chData.forEach((ch) => {
      const chEl = nodeRefs.current.get(ch.key);
      if (!chEl) return;
      const p1 = center(chEl, 'l');
      const p1r = center(chEl, 'r');
      const mx = (p0.x + p1.x) / 2;
      paths.push({ key: ch.key, d: `M ${p0.x} ${p0.y} C ${mx} ${p0.y},${mx} ${p1.y},${p1.x} ${p1.y}` });
      ch.endpoints.forEach((ep) => {
        const epEl = nodeRefs.current.get(ep.key);
        if (!epEl) return;
        const p2 = center(epEl, 'l');
        const mx2 = (p1r.x + p2.x) / 2;
        paths.push({ key: ep.key, d: `M ${p1r.x} ${p1r.y} C ${mx2} ${p1r.y},${mx2} ${p2.y},${p2.x} ${p2.y}` });
      });
    });

    const svg = svgRef.current;
    if (!svg) return;
    svg.innerHTML = '';
    pathRefs.current.clear();
    paths.forEach((p) => {
      const el = document.createElementNS('http://www.w3.org/2000/svg', 'path');
      el.setAttribute('d', p.d);
      el.setAttribute('fill', 'none');
      el.setAttribute('stroke', '#d8d7d1');
      el.setAttribute('stroke-width', '1.5');
      svg.appendChild(el);
      pathRefs.current.set(p.key, el);
    });
  }, [chData.length]);

  // Pulse + Ping on count changes
  useEffect(() => {
    if (modelTotal === 0) return;
    // Pick a random channel and endpoint to animate
    const ch = chData[Math.floor(Math.random() * chData.length)];
    const ep = ch?.endpoints[Math.floor(Math.random() * ch.endpoints.length)];

    const svg = svgRef.current;
    const id = ++pulseId.current;

    const pulse = (key: string, delay: number) => {
      setTimeout(() => {
        // Ping node
        const node = nodeRefs.current.get(key);
        if (node) { node.style.transform = 'scale(1.03)'; node.style.transition = 'transform 150ms'; setTimeout(() => { node.style.transform = 'scale(1)'; }, 200); }
        // Pulse dot along path
        const path = pathRefs.current.get(key);
        if (!path || !svg) return;
        const len = path.getTotalLength();
        const dot = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
        dot.setAttribute('r', '3.5'); dot.setAttribute('fill', '#4a7fc9');
        svg.appendChild(dot);
        const start = performance.now();
        const step = (now: number) => {
          const t = Math.min(1, (now - start) / 550);
          if (id !== pulseId.current) { dot.remove(); return; }
          const pt = path.getPointAtLength(t * len);
          dot.setAttribute('cx', String(pt.x)); dot.setAttribute('cy', String(pt.y));
          dot.setAttribute('opacity', String(1 - t * 0.3));
          if (t < 1) requestAnimationFrame(step); else dot.remove();
        };
        requestAnimationFrame(step);
      }, delay);
    };
    if (ch) { pulse(ch.key, 0); }
    if (ep) { pulse(ep.key, 200); }
  }, [modelTotal, chData.length]);

  return (
    <div style={{ background: '#fff', border: '1px solid #e4e3de', borderRadius: 10, padding: '20px 24px', marginBottom: 16, position: 'relative' }}>
      {/* Title */}
      <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 18, display: 'flex', alignItems: 'center', gap: 10 }}>
        <span>{m.model}</span>
        <span style={{ fontSize: 11, color: '#9a988f', background: '#f0efe9', padding: '1px 8px', borderRadius: 4, fontFamily: '"SF Mono",Consolas,monospace' }}>{m.pattern}</span>
        <span style={{ marginLeft: 'auto', fontSize: 12, color: '#6b6a64', fontVariantNumeric: 'tabular-nums' }}>
          共 <b style={{ color: '#1a1a18' }}>{modelTotal.toLocaleString()}</b> 次请求
        </span>
      </div>

      {/* Flow */}
      <div ref={containerRef} style={{ position: 'relative', display: 'grid', gridTemplateColumns: '200px 1fr 200px 1fr 200px', alignItems: 'center', minHeight: 60 }}>
        <svg ref={svgRef} style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%', pointerEvents: 'none', overflow: 'visible', zIndex: 0 }} />

        {/* Model column */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, zIndex: 1 }}>
          <div style={{ fontSize: 10.5, color: '#9a988f', textTransform: 'uppercase', letterSpacing: '0.04em', marginBottom: -2 }}>模型</div>
          <FlowNode refMap={nodeRefs} k={mk} title={m.model} sub={m.pattern} />
        </div>
        <div />

        {/* Channel column */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, zIndex: 1 }}>
          <div style={{ fontSize: 10.5, color: '#9a988f', textTransform: 'uppercase', letterSpacing: '0.04em', marginBottom: -2 }}>路由渠道（负载均衡）</div>
          {chData.map((ch) => {
            const lv = loadClass(ch.count, chCounts);
            return (
              <FlowNode
                key={ch.key} refMap={nodeRefs} k={ch.key}
                title={ch.label} count={ch.count}
                bar={chCounts.length > 1 ? (ch.count / Math.max(1, ...chCounts)) : 0}
                barColor={LOAD_COLORS[lv]}
                borderColor={LOAD_COLORS[lv]}
              />
            );
          })}
        </div>
        <div />

        {/* Endpoint column */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 10, zIndex: 1 }}>
          <div style={{ fontSize: 10.5, color: '#9a988f', textTransform: 'uppercase', letterSpacing: '0.04em', marginBottom: -2 }}>渠道端点（负载均衡）</div>
          {chData.map((ch) => {
            const epCounts = ch.endpoints.map((e) => e.count);
            return ch.endpoints.map((ep) => {
              const lv = loadClass(ep.count, epCounts);
              return (
                <FlowNode
                  key={ep.key} refMap={nodeRefs} k={ep.key}
                  title={ep.label} sub={ep.url} count={ep.count}
                  bar={epCounts.length > 1 ? (ep.count / Math.max(1, ...epCounts)) : 0}
                  barColor={LOAD_COLORS[lv]}
                  borderColor={LOAD_COLORS[lv]}
                />
              );
            });
          })}
        </div>
      </div>
    </div>
  );
}

function FlowNode({ refMap, k, title, sub, count, bar, barColor, borderColor }: {
  refMap: React.MutableRefObject<Map<string, HTMLDivElement>>;
  k: string; title: string; sub?: string; count?: number;
  bar?: number; barColor?: string; borderColor?: string;
}) {
  return (
    <div
      ref={(el) => { if (el) refMap.current.set(k, el); }}
      data-key={k}
      style={{
        border: `1.5px solid ${borderColor || '#e4e3de'}`,
        borderRadius: 8, padding: '9px 12px',
        background: '#fafaf8', fontSize: 12.5,
        transition: 'background-color 0.4s, border-color 0.4s, transform 0.15s',
      }}
    >
      <div style={{ fontWeight: 600, display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
        <span>{title}</span>
        {count !== undefined && <span style={{ fontSize: 12, fontVariantNumeric: 'tabular-nums', color: '#6b6a64' }}>{count}</span>}
      </div>
      {sub && <div style={{ fontSize: 10.5, color: '#9a988f', marginTop: 2 }}>{sub}</div>}
      {bar !== undefined && bar > 0 && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: '#eeede8', overflow: 'hidden' }}>
          <div style={{ height: '100%', borderRadius: 2, background: barColor || '#4a7fc9', width: `${Math.max(2, bar * 100)}%`, transition: 'width 0.4s, background-color 0.4s' }} />
        </div>
      )}
    </div>
  );
}
