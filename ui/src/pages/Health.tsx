import { useEffect, useRef } from 'react';
import { useRoutingHealth } from '@/api/health';
import { Card } from '@/components/ui/card';

/* ── Pure vanilla-JS flow panel, same pattern as the reference HTML ── */

function keyOf(...parts: (string | number)[]) { return parts.join('>'); }

function loadClass(cnt: number, siblings: number[]): 'low' | 'mid' | 'high' {
  const max = Math.max(1, ...siblings);
  const r = cnt / max;
  if (r >= 0.66) return 'high';
  if (r >= 0.33) return 'mid';
  return 'low';
}

export default function HealthPage() {
  const { data } = useRoutingHealth();
  const summary = data?.summary;
  const panelsRef = useRef<HTMLDivElement>(null);
  const stateRef = useRef<Record<string, number>>({});
  const totalRef = useRef(0);
  const topologyRef = useRef<any[]>([]);

  const models = data?.models ?? [];

  // Build topology once
  useEffect(() => {
    if (!models.length) return;
    topologyRef.current = models.map((m: any) => ({
      model: m.name,
      pattern: m.model_pattern,
      channels: m.channels.map((ch: any) => ({
        id: ch.channel_id,
        label: ch.channel_name || ch.channel_id,
        weight: Math.max(1, ch.requests || 1),
        endpoints: (ch.endpoints || []).map((ep: any) => ({
          id: `端点 ${ep.endpoint_id}`,
          url: ep.url || '',
          weight: Math.max(1, ep.available ? 1 : 0.1),
        })),
        ...((ch.endpoints?.length ? {} : { endpoints: [{ id: '端点 1', url: '', weight: 1 }] })),
      })),
    }));

    renderPanels();
  }, [models]);

  // WebSocket
  useEffect(() => {
    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${proto}//${window.location.host}/admin/api/health/ws`;
    let ws: WebSocket;

    function connect() {
      ws = new WebSocket(url);
      ws.onmessage = (e) => {
        try {
          const ev = JSON.parse(e.data);
          if (!ev.model || !ev.channel_id) return;

          const s = stateRef.current;
          const mk = keyOf('m', ev.model);
          s[mk] = (s[mk] || 0) + 1;
          const ck = keyOf('c', ev.model, ev.channel_id);
          s[ck] = (s[ck] || 0) + 1;
          if (ev.endpoint_id) {
            const ek = keyOf('e', ev.model, ev.channel_id, ev.endpoint_id);
            s[ek] = (s[ek] || 0) + 1;
          }
          totalRef.current++;

          // Update counters and bars via direct DOM
          updateDOM();

          // Pulse animation
          const panel = document.querySelector(`[data-model="${ev.model}"]`);
          if (panel) {
            const svg = panel.querySelector('svg.connectors') as SVGSVGElement;
            if (svg) {
              const cKey = keyOf('c', ev.model, ev.channel_id);
              pulseAlongPath(svg, cKey);
              pingNode(panel as HTMLElement, mk);
              setTimeout(() => pingNode(panel as HTMLElement, cKey), 150);
              if (ev.endpoint_id) {
                const eKey = keyOf('e', ev.model, ev.channel_id, ev.endpoint_id);
                setTimeout(() => pulseAlongPath(svg, eKey), 200);
                setTimeout(() => pingNode(panel as HTMLElement, eKey), 350);
              }
            }
          }

          document.getElementById('totalCount')!.textContent = totalRef.current.toLocaleString();
        } catch { /* ignore */ }
      };
      ws.onclose = () => setTimeout(connect, 3000);
      ws.onerror = () => ws.close();
    }
    connect();
    return () => { try { ws.close(); } catch {} };
  }, []);

  function updateDOM() {
    const s = stateRef.current;
    const panels = document.getElementById('panels');
    if (!panels) return;

    topologyRef.current.forEach((m: any) => {
      const panel = panels.querySelector(`[data-model="${m.model}"]`) as HTMLElement;
      if (!panel) return;

      // Model total
      const mk = keyOf('m', m.model);
      const mt = panel.querySelector('.model-total');
      if (mt) mt.textContent = (s[mk] || 0).toLocaleString();

      const chCounts = m.channels.map((c: any) => s[keyOf('c', m.model, c.id)] || 0);

      m.channels.forEach((c: any) => {
        const ck = keyOf('c', m.model, c.id);
        const cnt = s[ck] || 0;
        const node = panel.querySelector(`[data-key="${ck}"]`) as HTMLElement;
        if (!node) return;

        const nCount = node.querySelector('.n-count');
        if (nCount) nCount.textContent = String(cnt);

        const cls = loadClass(cnt, chCounts);
        const fill = node.querySelector('.n-bar-fill') as HTMLElement;
        if (fill) {
          fill.className = 'n-bar-fill load-' + cls;
          const max = Math.max(1, ...chCounts);
          fill.style.width = Math.round((cnt / max) * 100) + '%';
        }
        node.classList.remove('hl-low', 'hl-mid', 'hl-high');
        node.classList.add('hl-' + cls);

        const epCounts = c.endpoints.map((e: any) => s[keyOf('e', m.model, c.id, e.id)] || 0);
        c.endpoints.forEach((e: any) => {
          const ek = keyOf('e', m.model, c.id, e.id);
          const ecnt = s[ek] || 0;
          const enode = panel.querySelector(`[data-key="${ek}"]`) as HTMLElement;
          if (!enode) return;

          const eCount = enode.querySelector('.n-count');
          if (eCount) eCount.textContent = String(ecnt);

          const ecls = loadClass(ecnt, epCounts);
          const efill = enode.querySelector('.n-bar-fill') as HTMLElement;
          if (efill) {
            efill.className = 'n-bar-fill load-' + ecls;
            const emax = Math.max(1, ...epCounts);
            efill.style.width = Math.round((ecnt / emax) * 100) + '%';
          }
          enode.classList.remove('hl-low', 'hl-mid', 'hl-high');
          enode.classList.add('hl-' + ecls);
        });
      });
    });
  }

  function renderPanels() {
    const container = document.getElementById('panels');
    if (!container) return;

    container.innerHTML = topologyRef.current.map((m: any) => {
      const chNodes = m.channels.map((c: any) => `
        <div class="flow-node" data-key="${keyOf('c', m.model, c.id)}">
          <div class="n-title"><span>${c.label}</span><span class="n-count">0</span></div>
          <div class="n-bar-track"><div class="n-bar-fill load-low" style="width:0%"></div></div>
        </div>`).join('');

      const epNodes = m.channels.flatMap((c: any) => c.endpoints.map((e: any) => `
        <div class="flow-node" data-key="${keyOf('e', m.model, c.id, e.id)}" data-channel="${c.id}">
          <div class="n-title"><span>${e.id}</span><span class="n-count">0</span></div>
          <div class="n-sub">${e.url}<span class="channel-tag">${c.id}</span></div>
          <div class="n-bar-track"><div class="n-bar-fill load-low" style="width:0%"></div></div>
        </div>`)).join('');

      return `<div class="model-panel" data-model="${m.model}">
        <div class="model-panel-title">
          <span>${m.model}</span>
          <span class="pattern">${m.pattern}</span>
          <span class="count">共 <b class="model-total">0</b> 次请求</span>
        </div>
        <div class="flow-container">
          <svg class="connectors"></svg>
          <div class="col col-model">
            <div class="col-label">模型</div>
            <div class="flow-node" data-key="${keyOf('m', m.model)}">
              <div class="n-title"><span>${m.model}</span><span class="n-count">0</span></div>
            </div>
          </div>
          <div></div>
          <div class="col col-channel">
            <div class="col-label">路由渠道（负载均衡）</div>
            ${chNodes}
          </div>
          <div></div>
          <div class="col col-endpoint">
            <div class="col-label">渠道端点（负载均衡）</div>
            ${epNodes}
          </div>
        </div>
      </div>`;
    }).join('');

    // Draw connectors after render
    setTimeout(() => drawAllConnectors(), 50);
    window.addEventListener('resize', drawAllConnectors);
  }

  function drawAllConnectors() {
    document.querySelectorAll('.model-panel').forEach((panel) => {
      const m = topologyRef.current.find((t: any) => t.model === (panel as HTMLElement).dataset.model);
      if (!m) return;
      const container = panel.querySelector('.flow-container') as HTMLElement;
      const svg = panel.querySelector('svg.connectors') as SVGSVGElement;
      if (!container || !svg) return;
      const box = container.getBoundingClientRect();
      svg.innerHTML = '';

      function center(el: HTMLElement, side: 'l' | 'r') {
        const r = el.getBoundingClientRect();
        return { x: side === 'r' ? r.right - box.left : r.left - box.left, y: r.top + r.height / 2 - box.top };
      }

      const modelNode = container.querySelector(`.col-model .flow-node`) as HTMLElement;
      if (!modelNode) return;
      const p0 = center(modelNode, 'r');

      m.channels.forEach((c: any) => {
        const chNode = container.querySelector(`[data-key="${keyOf('c', m.model, c.id)}"]`) as HTMLElement;
        if (!chNode) return;
        const p1 = center(chNode, 'l');
        const p1r = center(chNode, 'r');
        addPath(svg, p0, p1, keyOf('c', m.model, c.id));
        c.endpoints.forEach((e: any) => {
          const epNode = container.querySelector(`[data-key="${keyOf('e', m.model, c.id, e.id)}"]`) as HTMLElement;
          if (!epNode) return;
          const p2 = center(epNode, 'l');
          addPath(svg, p1r, p2, keyOf('e', m.model, c.id, e.id));
        });
      });
    });
  }

  function addPath(svg: SVGSVGElement, p0: {x: number, y: number}, p1: {x: number, y: number}, key: string) {
    const mx = (p0.x + p1.x) / 2;
    const d = `M ${p0.x} ${p0.y} C ${mx} ${p0.y},${mx} ${p1.y},${p1.x} ${p1.y}`;
    const el = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    el.setAttribute('d', d);
    el.setAttribute('fill', 'none');
    el.setAttribute('stroke', '#d8d7d1');
    el.setAttribute('stroke-width', '1.5');
    el.setAttribute('data-path-key', key);
    svg.appendChild(el);
  }

  const summaryData = [
    ['总请求数 / 24h', summary?.total_requests_24h?.toLocaleString() ?? '-'],
    ['整体成功率', summary ? `${(summary.overall_success_rate * 100).toFixed(1)}%` : '-'],
    ['活跃渠道数', `${summary?.active_channels ?? '-'}`],
    ['熔断中渠道', `${summary?.broken_channels ?? '-'}`],
  ];

  return (
    <div style={{ fontFamily: '-apple-system,"PingFang SC","Microsoft YaHei",Segoe UI,sans-serif', color: '#1a1a18' }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>实时路由流量面板</h1>
      <p style={{ fontSize: 13, color: '#6b6a64', margin: '0 0 20px' }}>
        模型&nbsp;→&nbsp;路由渠道（负载均衡）→&nbsp;渠道端点（负载均衡），颜色表示相对负载：
        <span style={{ color: '#4a7fc9' }}> 蓝=低</span>
        <span style={{ color: '#d99a2b' }}> · 黄=中</span>
        <span style={{ color: '#c94a4a' }}> · 红=高</span>
      </p>

      <div className="top-bar" style={{ display: 'flex', alignItems: 'center', gap: 16, marginBottom: 20 }}>
        <div className="live-badge" style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: '#1a8a3d' }}>
          <span className="live-dot" style={{ width: 7, height: 7, borderRadius: '50%', background: '#1a8a3d', boxShadow: '0 0 0 0 rgba(26,138,61,0.5)', animation: 'pulse-dot 1.6s infinite' }} />
          LIVE
        </div>
        <div className="total-counter" style={{ fontSize: 12, color: '#6b6a64' }}>
          总请求数 <b id="totalCount" style={{ fontSize: 15, color: '#1a1a18', fontVariantNumeric: 'tabular-nums' }}>0</b>
        </div>
        <div className="legend" style={{ display: 'flex', gap: 16, marginLeft: 'auto', fontSize: 11.5, color: '#6b6a64' }}>
          {[{ c: '#4a7fc9', l: '低负载' }, { c: '#d99a2b', l: '中负载' }, { c: '#c94a4a', l: '高负载' }].map((x) => (
            <span key={x.l} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
              <span style={{ width: 22, height: 6, borderRadius: 3, background: x.c }} />{x.l}
            </span>
          ))}
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 20 }}>
        {summaryData.map(([label, val]) => (
          <Card key={label as string} style={{ padding: '14px 16px' }}>
            <div style={{ fontSize: 12, color: '#6b6a64', marginBottom: 6 }}>{label}</div>
            <div style={{ fontSize: 22, fontWeight: 600 }}>{val as string}</div>
          </Card>
        ))}
      </div>

      <div id="panels" ref={panelsRef} />
    </div>
  );
}

function pulseAlongPath(svg: SVGSVGElement, key: string) {
  const path = svg.querySelector(`path[data-path-key="${key}"]`) as SVGPathElement;
  if (!path) return;
  const len = path.getTotalLength();
  const dot = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
  dot.setAttribute('r', '3.5'); dot.setAttribute('fill', '#4a7fc9');
  svg.appendChild(dot);
  const dur = 550;
  const start = performance.now();
  function step(now: number) {
    const t = Math.min(1, (now - start) / dur);
    const pt = path.getPointAtLength(t * len);
    dot.setAttribute('cx', String(pt.x)); dot.setAttribute('cy', String(pt.y));
    dot.setAttribute('opacity', String(1 - t * 0.3));
    if (t < 1) requestAnimationFrame(step); else dot.remove();
  }
  requestAnimationFrame(step);
}

function pingNode(panel: HTMLElement, key: string) {
  const node = panel.querySelector(`[data-key="${key}"]`) as HTMLElement;
  if (!node) return;
  node.classList.add('pinged');
  setTimeout(() => node.classList.remove('pinged'), 200);
}
