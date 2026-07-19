import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import { useRoutingHealth } from "@/api/health";
import { Card } from "@/components/ui/card";

function keyFor(...parts: (string | number)[]) { return parts.join(">"); }

function loadClass(cnt: number, siblings: number[]) {
  const max = Math.max(1, ...siblings);
  const r = cnt / max;
  if (r >= 0.66) return "high";
  if (r >= 0.33) return "mid";
  return "low";
}

const LOAD = { low: "#4a7fc9", mid: "#d99a2b", high: "#c94a4a" };
const BAR = { low: "25%", mid: "60%", high: "100%" };

// ── FlowNode ──
function FlowNode({ nodeRef, title, subtitle, count, loadCls, pinged, showBar = true }: any) {
  const color = loadCls ? LOAD[loadCls as keyof typeof LOAD] : undefined;
  const barW = showBar && loadCls ? BAR[loadCls as keyof typeof BAR] : "0%";
  return (
    <div ref={nodeRef} style={{
      borderRadius: 8, border: `1.5px solid ${color || "#e4e3de"}`,
      background: "#fafaf8", padding: "9px 12px", fontSize: 12.5,
      transition: "transform 150ms, border-color 300ms",
      transform: pinged ? "scale(1.03)" : "scale(1)",
    }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <span style={{ fontWeight: 600, color: color || "#1a1a18" }}>{title}</span>
        <span style={{ fontSize: 12, color: "#6b6a64", fontVariantNumeric: "tabular-nums" }}>{count}</span>
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: "#9a988f", marginTop: 2 }}>{subtitle}</div>}
      {showBar && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: "#eeede8", overflow: "hidden" }}>
          <div style={{ height: "100%", borderRadius: 2, width: barW, background: color || "transparent", transition: "width 400ms ease, background-color 400ms ease" }} />
        </div>
      )}
    </div>
  );
}

// ── FlowPulse ──
function FlowPulse({ pathD, onDone }: { pathD: string; onDone: () => void }) {
  const dotRef = useRef<SVGCircleElement>(null);
  const pathRef = useRef<SVGPathElement>(null);
  useEffect(() => {
    const el = pathRef.current;
    if (!el) return;
    const len = el.getTotalLength();
    const start = performance.now();
    let raf: number;
    function step(now: number) {
      const t = Math.min(1, (now - start) / 550);
      const pt = el!.getPointAtLength(t * len);
      if (dotRef.current) {
        dotRef.current.setAttribute("cx", String(pt.x));
        dotRef.current.setAttribute("cy", String(pt.y));
        dotRef.current.setAttribute("opacity", String(1 - t * 0.3));
      }
      if (t < 1) raf = requestAnimationFrame(step); else onDone();
    }
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [onDone]);
  return <><path ref={pathRef} d={pathD} fill="none" stroke="none" /><circle ref={dotRef} r="3.5" fill="#4a7fc9" /></>;
}

// ── ModelPanel ──
function ModelPanel({ m, counts, lastEvent }: { m: any; counts: Record<string, number>; lastEvent: any }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const modelRef = useRef<HTMLDivElement>(null);
  const chRefs = useRef<Record<string, any>>({});
  const epRefs = useRef<Record<string, any>>({});
  const [pulses, setPulses] = useState<any[]>([]);
  const [pinged, setPinged] = useState<Record<string, boolean>>({});
  const initRef = useRef(false);

  if (!initRef.current) {
    m.channels.forEach((c: any) => { chRefs.current[c.channel_id] ||= { current: null }; });
    m.channels.forEach((c: any) => c.endpoints?.forEach((e: any) => { const k = `${c.channel_id}>${e.endpoint_id}`; epRefs.current[k] ||= { current: null }; }));
    initRef.current = true;
  }

  const pairs = useMemo(() => {
    const p: any[] = [];
    m.channels.forEach((c: any) => {
      const chKey = keyFor(m.name, c.channel_id);
      p.push({ key: chKey, from: modelRef, to: chRefs.current[c.channel_id] });
      c.endpoints?.forEach((e: any) => {
        const epKey = keyFor(m.name, c.channel_id, e.endpoint_id);
        p.push({ key: epKey, from: chRefs.current[c.channel_id], to: epRefs.current[`${c.channel_id}>${e.endpoint_id}`] });
      });
    });
    return p;
  }, [m]);

  // SVG connectors
  const [paths, setPaths] = useState<any[]>([]);
  const svgRef = useRef<SVGSVGElement>(null);

  const draw = useCallback(() => {
    const box = containerRef.current?.getBoundingClientRect();
    if (!box) return;
    setPaths(pairs.map(({ key, from, to }) => {
      const f = from?.current?.getBoundingClientRect();
      const t = to?.current?.getBoundingClientRect();
      if (!f || !t) return null;
      const p0 = { x: f.right - box.left, y: f.top + f.height / 2 - box.top };
      const p1 = { x: t.left - box.left, y: t.top + t.height / 2 - box.top };
      const mx = (p0.x + p1.x) / 2;
      return { key, d: `M ${p0.x} ${p0.y} C ${mx} ${p0.y},${mx} ${p1.y},${p1.x} ${p1.y}` };
    }).filter(Boolean);
  }, [pairs]);

  useEffect(() => { draw(); window.addEventListener("resize", draw); return () => window.removeEventListener("resize", draw); }, [draw]);

  // Events
  useEffect(() => {
    if (!lastEvent || lastEvent.model !== m.name) return;
    const { channel, endpoint, ts } = lastEvent;
    const chP = paths.find((p: any) => p.key === keyFor(m.name, channel));
    const epP = endpoint ? paths.find((p: any) => p.key === keyFor(m.name, channel, endpoint)) : null;
    if (chP) setPulses((prev: any[]) => [...prev, { id: `${ts}-c`, d: chP.d }]);
    const t1 = epP ? setTimeout(() => setPulses((prev: any[]) => [...prev, { id: `${ts}-e`, d: epP.d }]), 200) : undefined;
    const ks = [keyFor(m.name), keyFor(m.name, channel)];
    if (endpoint) ks.push(keyFor(m.name, channel, endpoint));
    const ts2 = ks.map((k, i) => setTimeout(() => { setPinged((p) => ({ ...p, [k]: true })); setTimeout(() => setPinged((p) => ({ ...p, [k]: false })), 200); }, i * 150));
    return () => { clearTimeout(t1); ts2.forEach(clearTimeout); };
  }, [lastEvent]);

  const rmPulse = useCallback((id: string) => setPulses((prev) => prev.filter((p: any) => p.id !== id)), []);

  const mc = counts[keyFor(m.name)] || 0;
  const ccs = m.channels.map((c: any) => counts[keyFor(m.name, c.channel_id)] || 0);

  return (
    <div style={{ marginBottom: 16, borderRadius: 10, border: "1px solid #e4e3de", background: "#fff", padding: "20px 24px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 18, fontSize: 14, fontWeight: 600 }}>
        <span>{m.name}</span>
        <span style={{ fontSize: 11, color: "#9a988f", background: "#f0efe9", padding: "1px 8px", borderRadius: 4, fontFamily: "monospace" }}>{m.model_pattern}</span>
        <span style={{ marginLeft: "auto", fontSize: 12, color: "#6b6a64" }}>共 <b style={{ color: "#1a1a18" }}>{mc}</b> 次请求</span>
      </div>
      <div ref={containerRef} style={{ position: "relative", display: "grid", gridTemplateColumns: "200px 1fr 200px 1fr 200px", alignItems: "center", minHeight: 60 }}>
        <svg ref={svgRef} style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%", overflow: "visible", pointerEvents: "none" }}>
          {paths.map((p: any) => <path key={p.key} d={p.d} fill="none" stroke="#d8d7d1" strokeWidth="1.5" />)}
          {pulses.map((p: any) => <FlowPulse key={p.id} pathD={p.d} onDone={() => rmPulse(p.id)} />)}
        </svg>

        {/* Model */}
        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: "#9a988f", textTransform: "uppercase", letterSpacing: "0.04em" }}>模型</div>
          <FlowNode nodeRef={modelRef} title={m.name} count={mc} pinged={!!pinged[keyFor(m.name)]} showBar={false} />
        </div>
        <div />

        {/* Channels */}
        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: "#9a988f", textTransform: "uppercase", letterSpacing: "0.04em" }}>路由渠道（负载均衡）</div>
          {m.channels.map((c: any) => {
            const cnt = counts[keyFor(m.name, c.channel_id)] || 0;
            const cls = loadClass(cnt, ccs);
            return <FlowNode key={c.channel_id} nodeRef={chRefs.current[c.channel_id]} title={c.channel_name || c.channel_id} count={cnt} loadCls={cls} pinged={!!pinged[keyFor(m.name, c.channel_id)]} />;
          })}
        </div>
        <div />

        {/* Endpoints */}
        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: "#9a988f", textTransform: "uppercase", letterSpacing: "0.04em" }}>渠道端点（负载均衡）</div>
          {m.channels.flatMap((c: any) => {
            const epCnt = (c.endpoints || []).map((e: any) => counts[keyFor(m.name, c.channel_id, e.endpoint_id)] || 0);
            return (c.endpoints || []).map((e: any) => {
              const k = keyFor(m.name, c.channel_id, e.endpoint_id);
              return <FlowNode key={k} nodeRef={epRefs.current[`${c.channel_id}>${e.endpoint_id}`]} title={`端点 ${e.endpoint_id}`} subtitle={`${e.url || ""} · ${c.channel_id}`} count={counts[k] || 0} loadCls={loadClass(counts[k] || 0, epCnt)} pinged={!!pinged[k]} />;
            });
          })}
        </div>
      </div>
    </div>
  );
}

// ── Hook ──
function useStream(topology: any[]) {
  const [total, setTotal] = useState(0);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [ev, setEv] = useState<any>(null);
  const seen = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (!topology.length) return;
    const url = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/admin/api/health/ws`;
    let ws: WebSocket;
    function connect() {
      ws = new WebSocket(url);
      ws.onmessage = (e) => {
        try {
          const d = JSON.parse(e.data);
          if (!d.model || !d.channel_id) return;
          const id = `${d.timestamp || ""}-${d.model}-${d.channel_id}-${d.endpoint_id ?? ""}`;
          if (seen.current.has(id)) return;
          seen.current.add(id);
          if (seen.current.size > 500) seen.current = new Set([...seen.current].slice(-250));
          const ep = d.endpoint_id ? `端点 ${d.endpoint_id}` : undefined;
          setCounts((p) => ({ ...p, [keyFor(d.model)]: (p[keyFor(d.model)] || 0) + 1, [keyFor(d.model, d.channel_id)]: (p[keyFor(d.model, d.channel_id)] || 0) + 1, ...(ep ? { [keyFor(d.model, d.channel_id, ep)]: (p[keyFor(d.model, d.channel_id, ep)] || 0) + 1 } : {}) }));
          setTotal((c) => c + 1);
          setEv({ model: d.model, channel: d.channel_id, endpoint: ep, ts: performance.now() });
        } catch {}
      };
      ws.onclose = () => setTimeout(connect, 3000);
      ws.onerror = () => ws.close();
    }
    connect();
    return () => ws?.close();
  }, [topology]);

  return { counts, total, ev };
}

// ── Page ──
export default function HealthPage() {
  const { data: rd } = useRoutingHealth();
  const topo = useMemo(() => (rd?.models ?? []).map((m: any) => ({ ...m })), [rd]);
  const { counts, total, ev } = useStream(topo);
  const s = rd?.summary;

  return (
    <div className="space-y-6 animate-fade-in">
      <h1 className="text-2xl font-bold tracking-tight">实时路由流量面板</h1>
      <p className="text-sm text-muted-foreground">
        模型 → 路由渠道（负载均衡）→ 渠道端点（负载均衡），颜色表示相对负载：
        <span style={{ color: "#4a7fc9" }}> 蓝=低</span>
        <span style={{ color: "#d99a2b" }}> · 黄=中</span>
        <span style={{ color: "#c94a4a" }}> · 红=高</span>
      </p>

      <div className="flex items-center gap-4 text-sm">
        <div className="flex items-center gap-1.5 text-xs font-semibold text-green-600">
          <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />LIVE
        </div>
        <span className="text-muted-foreground tabular-nums">总请求数 <b className="text-foreground">{total.toLocaleString()}</b></span>
        <div className="flex gap-3 ml-auto text-xs text-muted-foreground">
          {[{ c: "#4a7fc9", l: "低负载" }, { c: "#d99a2b", l: "中负载" }, { c: "#c94a4a", l: "高负载" }].map((x) => (
            <span key={x.l} className="flex items-center gap-1.5"><span className="inline-block w-4 h-1.5 rounded" style={{ background: x.c }} />{x.l}</span>
          ))}
        </div>
      </div>

      {s && (
        <div className="grid grid-cols-4 gap-3">
          {[["总请求数 / 24h", s.total_requests_24h.toLocaleString()], ["整体成功率", `${(s.overall_success_rate * 100).toFixed(1)}%`], ["活跃渠道数", `${s.active_channels}`], ["熔断中渠道", `${s.broken_channels}`]].map(([l, v]) => (
            <Card key={l} className="p-4"><div className="text-xs text-muted-foreground mb-1">{l}</div><div className="text-xl font-semibold">{v}</div></Card>
          ))}
        </div>
      )}

      {topo.map((m: any) => <ModelPanel key={m.id || m.name} m={m} counts={counts} lastEvent={ev} />)}
    </div>
  );
}
