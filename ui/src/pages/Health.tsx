import { useState, useEffect, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoutingHealth, useRecentPaths } from '@/api/health';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { RefreshCw, Activity } from 'lucide-react';
import { cn } from '@/lib/utils';

type LoadLevel = 'low' | 'mid' | 'high';

const LOAD_COLORS: Record<LoadLevel, string> = { low: '#4a7fc9', mid: '#d99a2b', high: '#c94a4a' };
const LOAD_BG: Record<LoadLevel, string> = { low: 'bg-blue-50/30', mid: 'bg-yellow-50/30', high: 'bg-red-50/30' };
const LOAD_BORDER: Record<LoadLevel, string> = { low: 'border-[#4a7fc9]', mid: 'border-[#d99a2b]', high: 'border-[#c94a4a]' };
const LOAD_BAR: Record<LoadLevel, string> = { low: 'bg-[#4a7fc9]', mid: 'bg-[#d99a2b]', high: 'bg-[#c94a4a]' };

function loadLevel(count: number, siblings: number[]): LoadLevel {
  const max = Math.max(1, ...siblings);
  const ratio = count / max;
  if (ratio >= 0.66) return 'high';
  if (ratio >= 0.33) return 'mid';
  return 'low';
}

interface PathHitEvent {
  id: string;
  model: string;
  mk: string;
  ck: string;
  ek?: string;
}

export default function HealthPage() {
  const { t } = useTranslation();
  const { data, isLoading, isError, refetch } = useRoutingHealth();
  const summary = data?.summary;
  const models = data?.models ?? [];

  const { data: pathsData } = useRecentPaths();
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [events, setEvents] = useState<PathHitEvent[]>([]);
  const seenPaths = useRef<Set<string>>(new Set());

  useEffect(() => {
    if (!pathsData?.paths) return;
    const newEvents: PathHitEvent[] = [];

    setCounts((prev) => {
      const next = { ...prev };
      for (const req of pathsData.paths) {
        const uid = `${req.timestamp}-${req.model}-${req.channel_id}-${req.endpoint_id ?? ''}`;
        if (seenPaths.current.has(uid)) continue;
        seenPaths.current.add(uid);

        const mk = `m:${req.model}`;
        const ck = `c:${req.model}:${req.channel_id}`;
        next[mk] = (next[mk] || 0) + 1;
        next[ck] = (next[ck] || 0) + 1;

        let ek: string | undefined;
        if (req.endpoint_id) {
          ek = `e:${req.model}:${req.channel_id}:${req.endpoint_id}`;
          next[ek] = (next[ek] || 0) + 1;
        }

        newEvents.push({ id: uid, model: req.model, mk, ck, ek });
      }
      return next;
    });

    if (newEvents.length > 0) {
      setEvents((prev) => [...prev, ...newEvents].slice(-60));
    }

    if (seenPaths.current.size > 500) {
      seenPaths.current = new Set([...seenPaths.current].slice(-250));
    }
  }, [pathsData]);

  const totalRealtime = Object.values(counts).reduce((a, b) => a + b, 0);
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

  return (
    <div className="space-y-6 animate-fade-in">
      <div className="flex items-end justify-between flex-wrap gap-4">
        <div>
          <div className="text-xs font-mono tracking-wider text-primary mb-1.5 flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-green-500 shadow-[0_0_6px_rgba(34,197,94,0.5)] animate-pulse" />
            LIVE
          </div>
          <h1 className="text-2xl font-bold tracking-tight">实时路由流量面板</h1>
          <p className="text-sm text-muted-foreground mt-1">
            模型 → 路由渠道 → 渠道端点，颜色表示相对负载：
            <span style={{ color: LOAD_COLORS.low }}> 蓝=低</span>
            <span style={{ color: LOAD_COLORS.mid }}> 黄=中</span>
            <span style={{ color: LOAD_COLORS.high }}> 红=高</span>
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
        </Button>
      </div>

      <div className="grid grid-cols-4 gap-3">
        <Card><CardContent className="p-4">
          <p className="text-xs text-muted-foreground">总请求数 / 24h</p>
          <p className="text-xl font-semibold mt-1">{summary?.total_requests_24h?.toLocaleString() ?? '-'}</p>
        </CardContent></Card>
        <Card><CardContent className="p-4">
          <p className="text-xs text-muted-foreground">整体成功率</p>
          <p className={cn('text-xl font-semibold mt-1', (summary?.overall_success_rate ?? 1) > 0.9 ? 'text-green-600' : 'text-yellow-500')}>
            {summary ? pct(summary.overall_success_rate) : '-'}
          </p>
        </CardContent></Card>
        <Card><CardContent className="p-4">
          <p className="text-xs text-muted-foreground">活跃渠道数</p>
          <p className="text-xl font-semibold mt-1 text-green-600">{summary?.active_channels ?? '-'}</p>
        </CardContent></Card>
        <Card><CardContent className="p-4">
          <p className="text-xs text-muted-foreground">熔断中渠道</p>
          <p className={cn('text-xl font-semibold mt-1', (summary?.broken_channels ?? 0) > 0 ? 'text-yellow-500' : 'text-muted-foreground')}>
            {summary?.broken_channels ?? '-'}
          </p>
        </CardContent></Card>
      </div>

      <div className="flex items-center gap-4 text-sm">
        <div className="flex items-center gap-2 text-xs font-semibold text-green-600">
          <span className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
          实时流量
        </div>
        <span className="text-muted-foreground tabular-nums">
          本轮会话累计 <b className="text-foreground">{totalRealtime.toLocaleString()}</b> 次请求
        </span>
        <div className="flex gap-3 ml-auto text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5"><span className="inline-block w-4 h-1.5 rounded bg-[#4a7fc9]" /> 低负载</span>
          <span className="flex items-center gap-1.5"><span className="inline-block w-4 h-1.5 rounded bg-[#d99a2b]" /> 中负载</span>
          <span className="flex items-center gap-1.5"><span className="inline-block w-4 h-1.5 rounded bg-[#c94a4a]" /> 高负载</span>
        </div>
      </div>

      {isLoading ? (
        <div className="p-12 text-center text-sm text-muted-foreground">加载中...</div>
      ) : isError ? (
        <div className="p-12 text-center">
          <p className="text-sm text-destructive mb-3">加载失败</p>
          <Button variant="outline" onClick={() => refetch()}>重试</Button>
        </div>
      ) : models.length === 0 ? (
        <div className="p-16 text-center text-muted-foreground">
          <Activity className="w-10 h-10 mx-auto mb-3 opacity-50" />
          <div className="text-sm">暂无路由数据</div>
        </div>
      ) : (
        <div className="space-y-4">
          {models.map((m) => (
            <ModelPanel key={m.id} model={m} counts={counts} events={events} />
          ))}
        </div>
      )}
    </div>
  );
}

interface PathDef {
  key: string;
  d: string;
}

function ModelPanel({ model, counts, events }: { model: any; counts: Record<string, number>; events: PathHitEvent[] }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const [paths, setPaths] = useState<PathDef[]>([]);

  const pathRefs = useRef<Map<string, SVGPathElement>>(new Map());
  const nodeRefs = useRef<Map<string, HTMLElement>>(new Map());
  const processedEvents = useRef<Set<string>>(new Set());

  const mk = `m:${model.id}`;
  const modelCount = counts[mk] || 0;
  const chData = model.channels.map((ch: any) => {
    const ck = `c:${model.id}:${ch.channel_id}`;
    const epData = (ch.endpoints || []).map((ep: any) => ({
      key: `e:${model.id}:${ch.channel_id}:${ep.endpoint_id}`,
      label: `#${ep.endpoint_id}`,
      url: ep.url || '',
      count: counts[`e:${model.id}:${ch.channel_id}:${ep.endpoint_id}`] || 0,
    }));
    return { key: ck, label: ch.channel_name || ch.channel_id, count: counts[ck] || 0, endpoints: epData };
  });
  const chCounts = chData.map((c: any) => c.count);

  const redraw = useCallback(() => {
    if (!containerRef.current) return;
    const box = containerRef.current.getBoundingClientRect();

    const modelEl = nodeRefs.current.get(mk);
    if (!modelEl) return;

    const mr = modelEl.getBoundingClientRect();
    const p0 = { x: mr.right - box.left, y: mr.top + mr.height / 2 - box.top };

    const result: PathDef[] = [];
    chData.forEach((ch: any) => {
      const chEl = nodeRefs.current.get(ch.key);
      if (!chEl) return;
      const cr = chEl.getBoundingClientRect();
      const p1 = { x: cr.left - box.left, y: cr.top + cr.height / 2 - box.top };
      const p1r = { x: cr.right - box.left, y: cr.top + cr.height / 2 - box.top };
      const mx = (p0.x + p1.x) / 2;
      result.push({ key: ch.key, d: `M ${p0.x} ${p0.y} C ${mx} ${p0.y}, ${mx} ${p1.y}, ${p1.x} ${p1.y}` });

      ch.endpoints.forEach((ep: any) => {
        const epEl = nodeRefs.current.get(ep.key);
        if (!epEl) return;
        const er = epEl.getBoundingClientRect();
        const p2 = { x: er.left - box.left, y: er.top + er.height / 2 - box.top };
        const mx2 = (p1r.x + p2.x) / 2;
        result.push({ key: ep.key, d: `M ${p1r.x} ${p1r.y} C ${mx2} ${p1r.y}, ${mx2} ${p2.y}, ${p2.x} ${p2.y}` });
      });
    });
    setPaths(result);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model.id, chData.length]);

  useEffect(() => {
    redraw();
    window.addEventListener('resize', redraw);
    return () => window.removeEventListener('resize', redraw);
  }, [redraw]);

  const pingNode = useCallback((key: string) => {
    const el = nodeRefs.current.get(key);
    if (!el) return;
    el.style.transition = 'transform 150ms ease, box-shadow 150ms ease';
    el.style.transform = 'scale(1.035)';
    el.style.boxShadow = '0 0 0 2px rgba(74,127,201,0.25)';
    window.setTimeout(() => {
      el.style.transform = 'scale(1)';
      el.style.boxShadow = 'none';
    }, 180);
  }, []);

  const pulseAlongPath = useCallback((key: string, color: string) => {
    const path = pathRefs.current.get(key);
    const svg = svgRef.current;
    if (!path || !svg) return;
    const len = path.getTotalLength();
    const dot = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    dot.setAttribute('r', '3.5');
    dot.setAttribute('fill', color);
    svg.appendChild(dot);

    const duration = 550;
    const start = performance.now();
    function step(now: number) {
      const t = Math.min(1, (now - start) / duration);
      const pt = path.getPointAtLength(t * len);
      dot.setAttribute('cx', String(pt.x));
      dot.setAttribute('cy', String(pt.y));
      dot.setAttribute('opacity', String(1 - t * 0.3));
      if (t < 1) requestAnimationFrame(step);
      else dot.remove();
    }
    requestAnimationFrame(step);
  }, []);

  useEffect(() => {
    const relevant = events.filter((e) => e.model === model.id);
    for (const ev of relevant) {
      if (processedEvents.current.has(ev.id)) continue;
      processedEvents.current.add(ev.id);

      pingNode(ev.mk);
      pulseAlongPath(ev.ck, '#4a7fc9');
      window.setTimeout(() => pingNode(ev.ck), 150);

      if (ev.ek) {
        window.setTimeout(() => pulseAlongPath(ev.ek!, '#4a7fc9'), 200);
        window.setTimeout(() => pingNode(ev.ek!), 350);
      }
    }
    if (processedEvents.current.size > 300) {
      processedEvents.current = new Set([...processedEvents.current].slice(-150));
    }
  }, [events, model.id, pingNode, pulseAlongPath]);

  return (
    <Card>
      <div className="px-5 py-3.5 border-b bg-muted/20">
        <div className="flex items-center gap-2.5">
          <span className="font-semibold text-foreground">{model.name}</span>
          <span className="text-xs font-mono text-muted-foreground bg-muted px-2 py-0.5 rounded">{model.model_pattern}</span>
          <span className="ml-auto text-xs text-muted-foreground tabular-nums">
            共 <b className="text-foreground">{modelCount.toLocaleString()}</b> 次实时请求
          </span>
        </div>
      </div>
      <div className="p-5">
        <div ref={containerRef} className="relative" style={{ display: 'grid', gridTemplateColumns: '180px 1fr 180px 1fr 180px', alignItems: 'center', minHeight: 60 + Math.max(1, chData.length) * 56 }}>
          <svg ref={svgRef} className="absolute inset-0 w-full h-full pointer-events-none overflow-visible" style={{ zIndex: 0 }}>
            {paths.map((p) => (
              <path
                key={p.key}
                ref={(el) => {
                  if (el) pathRefs.current.set(p.key, el);
                  else pathRefs.current.delete(p.key);
                }}
                d={p.d}
                fill="none"
                stroke="#d8d7d1"
                strokeWidth="1.5"
              />
            ))}
          </svg>

          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">模型</div>
            <div
              data-n={mk}
              ref={(el) => { if (el) nodeRefs.current.set(mk, el); }}
              className="snk-model border-2 border-primary rounded-lg px-3 py-2.5 bg-primary/5 transition-transform"
            >
              <div className="text-sm font-semibold text-primary">{model.name}</div>
              <div className="text-[11px] text-muted-foreground">{model.model_pattern}</div>
            </div>
          </div>

          <div />

          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">路由渠道</div>
            {chData.map((ch: any) => {
              const lv = loadLevel(ch.count, chCounts);
              return (
                <div
                  key={ch.key}
                  data-n={ch.key}
                  ref={(el) => { if (el) nodeRefs.current.set(ch.key, el); }}
                  className={`border rounded-lg px-3 py-2 transition-all ${LOAD_BORDER[lv]} ${LOAD_BG[lv]}`}
                >
                  <div className="flex items-center justify-between text-[12.5px] font-semibold">
                    <span>{ch.label}</span>
                    <span className="text-xs text-muted-foreground tabular-nums">{ch.count}</span>
                  </div>
                  {chCounts.length > 1 && (
                    <div className="mt-1.5 h-1 bg-muted rounded-full overflow-hidden">
                      <div className={`h-full rounded-full transition-all duration-500 ${LOAD_BAR[lv]}`}
                        style={{ width: `${Math.max(2, (ch.count / Math.max(1, ...chCounts)) * 100)}%` }} />
                    </div>
                  )}
                </div>
              );
            })}
          </div>

          <div />

          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">渠道端点</div>
            {chData.map((ch: any) => {
              const epCounts = ch.endpoints.map((e: any) => e.count);
              return ch.endpoints.map((ep: any) => {
                const lv = loadLevel(ep.count, epCounts);
                return (
                  <div
                    key={ep.key}
                    data-n={ep.key}
                    ref={(el) => { if (el) nodeRefs.current.set(ep.key, el); }}
                    className={`border rounded-lg px-3 py-2 transition-all ${LOAD_BORDER[lv]} ${LOAD_BG[lv]}`}
                  >
                    <div className="flex items-center justify-between text-[12.5px] font-semibold">
                      <span>{ep.label}</span>
                      <span className="text-xs text-muted-foreground tabular-nums">{ep.count}</span>
                    </div>
                    {ep.url && <div className="text-[10px] text-muted-foreground truncate">{ep.url}</div>}
                    {epCounts.length > 1 && (
                      <div className="mt-1 h-1 bg-muted rounded-full overflow-hidden">
                        <div className={`h-full rounded-full transition-all duration-500 ${LOAD_BAR[lv]}`}
                          style={{ width: `${Math.max(2, (ep.count / Math.max(1, ...epCounts)) * 100)}%` }} />
                      </div>
                    )}
                  </div>
                );
              });
            })}
          </div>
        </div>
      </div>
    </Card>
  );
}
