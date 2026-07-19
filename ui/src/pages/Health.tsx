import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import { useRoutingHealth } from "@/api/health";
import { Card } from "@/components/ui/card";

const C = {
  bg: "#f5f5f3", cardBg: "#ffffff", border: "#e4e3de", line: "#d8d7d1",
  textPrimary: "#1a1a18", textSecondary: "#6b6a64", textMuted: "#9a988f",
  nodeBg: "#fafaf8", barTrack: "#eeede8", green: "#1a8a3d",
  low: "#4a7fc9", mid: "#d99a2b", high: "#c94a4a",
};

const LOAD_COLOR = { low: C.low, mid: C.mid, high: C.high };

const keyFor = (...parts) => parts.join(">");

function loadClass(count, siblingCounts) {
  const max = Math.max(1, ...siblingCounts);
  const r = count / max;
  if (r >= 0.66) return "high";
  if (r >= 0.33) return "mid";
  return "low";
}

// ── FlowNode ──
function FlowNode({ nodeRef, title, subtitle, count, loadCls, pinged, showBar = true }) {
  const color = loadCls ? LOAD_COLOR[loadCls] : null;
  const pcts = loadCls === "high" ? 100 : loadCls === "mid" ? 60 : 25;
  return (
    <div ref={nodeRef} style={{
      borderRadius: 8, border: `1.5px solid ${color || C.border}`, background: C.nodeBg,
      padding: "9px 12px", fontSize: 12.5,
      transition: "transform 150ms, border-color 300ms",
      transform: pinged ? "scale(1.03)" : "scale(1)",
    }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
        <span style={{ fontWeight: 600, color: color || C.textPrimary }}>{title}</span>
        <span style={{ fontSize: 12, color: C.textSecondary, fontVariantNumeric: "tabular-nums" }}>{count}</span>
      </div>
      {subtitle && <div style={{ fontSize: 10.5, color: C.textMuted, marginTop: 2 }}>{subtitle}</div>}
      {showBar && (
        <div style={{ marginTop: 6, height: 4, borderRadius: 2, background: C.barTrack, overflow: "hidden" }}>
          <div style={{ height: "100%", borderRadius: 2, width: `${pcts}%`, background: color || "transparent", transition: "width 400ms ease, background-color 400ms ease" }} />
        </div>
      )}
    </div>
  );
}

// ── FlowPulse ──
function FlowPulse({ pathD, duration = 550, onDone }) {
  const dotRef = useRef(null);
  const pathElRef = useRef(null);
  useEffect(() => {
    const pathEl = pathElRef.current;
    if (!pathEl) return;
    const len = pathEl.getTotalLength();
    const start = performance.now();
    let raf;
    function step(now) {
      const t = Math.min(1, (now - start) / duration);
      const pt = pathEl.getPointAtLength(t * len);
      if (dotRef.current) {
        dotRef.current.setAttribute("cx", pt.x);
        dotRef.current.setAttribute("cy", pt.y);
        dotRef.current.setAttribute("opacity", String(1 - t * 0.3));
      }
      if (t < 1) raf = requestAnimationFrame(step);
      else onDone();
    }
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [duration, onDone]);
  return <><path ref={pathElRef} d={pathD} fill="none" stroke="none" /><circle ref={dotRef} r="3.5" fill={C.low} /></>;
}

// ── useConnectors ──
function useConnectors(containerRef, pairs) {
  const svgRef = useRef(null);
  const [paths, setPaths] = useState([]);
  const recompute = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    const cRect = container.getBoundingClientRect();
    const next = pairs.map(({ key, fromRef, toRef }) => {
      const fromEl = fromRef.current, toEl = toRef.current;
      if (!fromEl || !toEl) return null;
      const fr = fromEl.getBoundingClientRect(), tr = toEl.getBoundingClientRect();
      const p0 = { x: fr.right - cRect.left, y: fr.top + fr.height / 2 - cRect.top };
      const p1 = { x: tr.left - cRect.left, y: tr.top + tr.height / 2 - cRect.top };
      const midX = (p0.x + p1.x) / 2;
      return { key, d: `M ${p0.x} ${p0.y} C ${midX} ${p0.y}, ${midX} ${p1.y}, ${p1.x} ${p1.y}` };
    }).filter(Boolean);
    setPaths(next);
  }, [containerRef, pairs]);
  useEffect(() => {
    recompute();
    const ro = new ResizeObserver(recompute);
    if (containerRef.current) ro.observe(containerRef.current);
    window.addEventListener("resize", recompute);
    return () => { ro.disconnect(); window.removeEventListener("resize", recompute); };
  }, [recompute, containerRef]);
  return { svgRef, paths };
}

// ── ModelPanel ──
function ModelPanel({ model, counts, lastEvent }) {
  const containerRef = useRef(null);
  const modelNodeRef = useRef(null);
  const channelRefs = useRef({});
  const endpointRefs = useRef({});
  const [pulses, setPulses] = useState([]);
  const [pinged, setPinged] = useState({});

  if (!channelRefs.current.initialized) {
    model.channels.forEach((c) => { channelRefs.current[c.id] ||= { current: null }; });
    model.channels.forEach((c) =>
      c.endpoints.forEach((e) => { const k = `${c.id}>${e.id}`; endpointRefs.current[k] ||= { current: null }; })
    );
    channelRefs.current.initialized = true;
  }

  const topology = model;
  const pairs = useMemo(() => {
    const p = [];
    topology.channels.forEach((c) => {
      p.push({ key: keyFor(topology.model, c.id), fromRef: modelNodeRef, toRef: channelRefs.current[c.id] });
      c.endpoints.forEach((e) => {
        p.push({ key: keyFor(topology.model, c.id, e.id), fromRef: channelRefs.current[c.id], toRef: endpointRefs.current[`${c.id}>${e.id}`] });
      });
    });
    return p;
  }, [topology]);

  const { svgRef, paths } = useConnectors(containerRef, pairs);

  useEffect(() => {
    if (!lastEvent || lastEvent.model !== topology.model) return;
    const { channel, endpoint, ts } = lastEvent;
    const chPath = paths.find((p) => p.key === keyFor(topology.model, channel));
    const epPath = paths.find((p) => p.key === keyFor(topology.model, channel, endpoint));
    if (chPath) setPulses((prev) => [...prev, { id: `${ts}-ch`, pathD: chPath.d }]);
    let epTimer;
    if (epPath) { epTimer = setTimeout(() => setPulses((prev) => [...prev, { id: `${ts}-ep`, pathD: epPath.d }]), 200); }
    const keys = [keyFor(topology.model), keyFor(topology.model, channel), keyFor(topology.model, channel, endpoint)];
    const timers = keys.map((k, i) => setTimeout(() => { setPinged((p) => ({ ...p, [k]: true })); setTimeout(() => setPinged((p) => ({ ...p, [k]: false })), 200); }, i * 150));
    return () => { clearTimeout(epTimer); timers.forEach(clearTimeout); };
  }, [lastEvent]);

  const removePulse = useCallback((id) => setPulses((prev) => prev.filter((p) => p.id !== id)), []);

  const modelCount = counts[keyFor(topology.model)] || 0;
  const chCounts = topology.channels.map((c) => counts[keyFor(topology.model, c.id)] || 0);

  return (
    <div style={{ marginBottom: 16, borderRadius: 10, border: `1px solid ${C.border}`, background: C.cardBg, padding: "20px 24px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 18, fontSize: 14, fontWeight: 600 }}>
        <span>{topology.model}</span>
        <span style={{ fontSize: 11, fontWeight: 400, color: C.textMuted, background: "#f0efe9", padding: "1px 8px", borderRadius: 4, fontFamily: "SF Mono, Consolas, monospace" }}>{topology.pattern}</span>
        <span style={{ marginLeft: "auto", fontSize: 12, fontWeight: 400, color: C.textSecondary }}>共 <b style={{ color: C.textPrimary, fontWeight: 600 }}>{modelCount}</b> 次请求</span>
      </div>

      <div ref={containerRef} style={{ position: "relative", display: "grid", gridTemplateColumns: "200px 1fr 200px 1fr 200px", alignItems: "center", minHeight: 60 }}>
        <svg ref={svgRef} style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%", overflow: "visible", pointerEvents: "none" }}>
          {paths.map((p) => <path key={p.key} d={p.d} fill="none" stroke={C.line} strokeWidth="1.5" />)}
          {pulses.map((p) => <FlowPulse key={p.id} pathD={p.pathD} onDone={() => removePulse(p.id)} />)}
        </svg>

        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: C.textMuted, textTransform: "uppercase", letterSpacing: "0.04em" }}>模型</div>
          <FlowNode nodeRef={modelNodeRef} title={topology.model} count={modelCount} pinged={pinged[keyFor(topology.model)]} showBar={false} />
        </div>
        <div />

        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: C.textMuted, textTransform: "uppercase", letterSpacing: "0.04em" }}>路由渠道（负载均衡）</div>
          {topology.channels.map((c) => {
            const cnt = counts[keyFor(topology.model, c.id)] || 0;
            return <FlowNode key={c.id} nodeRef={channelRefs.current[c.id]} title={c.id} count={cnt} loadCls={loadClass(cnt, chCounts)} pinged={pinged[keyFor(topology.model, c.id)]} />;
          })}
        </div>
        <div />

        <div style={{ zIndex: 1, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 10.5, color: C.textMuted, textTransform: "uppercase", letterSpacing: "0.04em" }}>渠道端点（负载均衡）</div>
          {topology.channels.flatMap((c) => {
            const epCounts = c.endpoints.map((e) => counts[keyFor(topology.model, c.id, e.id)] || 0);
            return c.endpoints.map((e) => {
              const k = keyFor(topology.model, c.id, e.id);
              return <FlowNode key={k} nodeRef={endpointRefs.current[`${c.id}>${e.id}`]} title={e.id} subtitle={`${e.url} · ${c.id}`} count={counts[k] || 0} loadCls={loadClass(counts[k] || 0, epCounts)} pinged={pinged[k]} />;
            });
          })}
        </div>
      </div>
    </div>
  );
}

// ── useRequestStream (WebSocket only, no simulation) ──
function useRequestStream(topology) {
  const [totalCount, setTotalCount] = useState(0);
  const [counts, setCounts] = useState({});
  const [lastEvent, setLastEvent] = useState(null);
  const seen = useRef(new Set());

  useEffect(() => {
    if (!topology.length) return;
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${window.location.host}/admin/api/health/ws`;

    function connect() {
      const ws = new WebSocket(url);
      ws.onmessage = (e) => {
        try {
          const ev = JSON.parse(e.data);
          if (!ev.model || !ev.channel_id) return;
          const uid = `${ev.timestamp || performance.now()}-${ev.model}-${ev.channel_id}-${ev.endpoint_id || ""}`;
          if (seen.current.has(uid)) return;
          seen.current.add(uid);
          if (seen.current.size > 500) seen.current = new Set([...seen.current].slice(-250));

          const endpoint = ev.endpoint_id ? `端点 ${ev.endpoint_id}` : undefined;
          setCounts((prev) => ({ ...prev, [keyFor(ev.model)]: (prev[keyFor(ev.model)] || 0) + 1, [keyFor(ev.model, ev.channel_id)]: (prev[keyFor(ev.model, ev.channel_id)] || 0) + 1, ...(endpoint ? { [keyFor(ev.model, ev.channel_id, endpoint)]: (prev[keyFor(ev.model, ev.channel_id, endpoint)] || 0) + 1 } : {}) }));
          setTotalCount((c) => c + 1);
          setLastEvent({ model: ev.model, channel: ev.channel_id, endpoint, ts: performance.now() });
        } catch { /* ignore */ }
      };
      ws.onclose = () => setTimeout(connect, 3000);
      ws.onerror = () => ws.close();
    }
    connect();
  }, [topology]);

  return { counts, totalCount, lastEvent };
}

// ── Top-level ──
export default function HealthPage() {
  const { data } = useRoutingHealth();
  const summary = data?.summary;

  const topology = useMemo(() => (data?.models ?? []).map((m) => ({
    model: m.name,
    pattern: m.model_pattern,
    channels: m.channels.map((ch) => ({
      id: ch.channel_id,
      weight: Math.max(1, ch.requests || 1),
      endpoints: (ch.endpoints || []).length > 0
        ? (ch.endpoints || []).map((ep) => ({ id: `端点 ${ep.endpoint_id}`, url: ep.url || "", weight: Math.max(1, ep.available ? 1 : 0.1) }))
        : [{ id: "端点 1", url: "", weight: 1 }],
    })),
  })), [data]);

  const { counts, totalCount, lastEvent } = useRequestStream(topology);

  return (
    <div style={{ background: C.bg, padding: 28, fontFamily: '-apple-system,"PingFang SC","Microsoft YaHei",Segoe UI,sans-serif', color: C.textPrimary }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: "0 0 4px" }}>实时路由流量面板</h1>
      <p style={{ fontSize: 13, color: C.textSecondary, margin: "0 0 20px" }}>
        模型 → 路由渠道（负载均衡）→ 渠道端点（负载均衡），颜色表示相对负载：
        <span style={{ color: C.low }}> 蓝=低</span> · <span style={{ color: C.mid }}> 黄=中</span> · <span style={{ color: C.high }}> 红=高</span>
      </p>

      <div style={{ display: "flex", alignItems: "center", gap: 16, marginBottom: 20 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, fontWeight: 600, color: C.green }}>
          <span style={{ width: 7, height: 7, borderRadius: "50%", background: C.green, boxShadow: "0 0 0 0 rgba(26,138,61,0.5)", animation: "gw-pulse 1.6s infinite" }} />LIVE
        </div>
        <div style={{ fontSize: 12, color: C.textSecondary }}>
          总请求数 <b style={{ fontSize: 15, color: C.textPrimary, fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>{totalCount.toLocaleString()}</b>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", gap: 16, fontSize: 11.5, color: C.textSecondary }}>
          {[{ c: C.low, l: "低负载" }, { c: C.mid, l: "中负载" }, { c: C.high, l: "高负载" }].map((x) => (
            <span key={x.l} style={{ display: "flex", alignItems: "center", gap: 5 }}><span style={{ width: 22, height: 6, borderRadius: 3, background: x.c }} />{x.l}</span>
          ))}
        </div>
      </div>

      {summary && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12, marginBottom: 20 }}>
          {[
            ["总请求数 / 24h", summary.total_requests_24h.toLocaleString()],
            ["整体成功率", `${(summary.overall_success_rate * 100).toFixed(1)}%`],
            ["活跃渠道数", `${summary.active_channels}`],
            ["熔断中渠道", `${summary.broken_channels}`],
          ].map(([label, val]) => (
            <Card key={label} style={{ padding: "14px 16px" }}>
              <div style={{ fontSize: 12, color: C.textSecondary, marginBottom: 6 }}>{label}</div>
              <div style={{ fontSize: 22, fontWeight: 600 }}>{val}</div>
            </Card>
          ))}
        </div>
      )}

      {topology.map((m) => <ModelPanel key={m.model} model={m} counts={counts} lastEvent={lastEvent} />)}

      <style>{`@keyframes gw-pulse { 0% { box-shadow: 0 0 0 0 rgba(26,138,61,0.5); } 70% { box-shadow: 0 0 0 6px rgba(26,138,61,0); } 100% { box-shadow: 0 0 0 0 rgba(26,138,61,0); } }`}</style>
    </div>
  );
}
