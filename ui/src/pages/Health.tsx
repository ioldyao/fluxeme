import { useState, useEffect, useRef, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useRoutingHealth } from '@/api/health';
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

export default function HealthPage() {
  const { t } = useTranslation();
  const { data, isLoading, isError, refetch } = useRoutingHealth();
  const summary = data?.summary;
  const models = data?.models ?? [];

  // Simulated real-time counts (increment on each simulated request)
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [pulseTrigger, setPulseTrigger] = useState(0);

  useEffect(() => {
    if (models.length === 0) return;
    const interval = setInterval(() => {
      const m = models[Math.floor(Math.random() * models.length)];
      if (!m.channels.length) return;
      const ch = m.channels[Math.floor(Math.random() * m.channels.length)];
      const ep = ch.endpoints.length > 0
        ? ch.endpoints[Math.floor(Math.random() * ch.endpoints.length)]
        : null;
      const mk = `m:${m.id}`;
      const ck = `c:${m.id}:${ch.channel_id}`;
      const ek = ep ? `e:${m.id}:${ch.channel_id}:${ep.endpoint_id}` : ck;
      setCounts((p) => ({ ...p, [mk]: (p[mk] || 0) + 1, [ck]: (p[ck] || 0) + 1, [ek]: (p[ek] || 0) + 1 }));
      setPulseTrigger((p) => p + 1);
    }, 800 + Math.random() * 1200);
    return () => clearInterval(interval);
  }, [models]);

  const totalRealtime = Object.values(counts).reduce((a, b) => a + b, 0);
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;

  return (
    <div className="space-y-6 animate-fade-in">
      {/* Header */}
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

      {/* Summary */}
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

      {/* Live counter bar */}
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

      {/* Content */}
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
            <ModelPanel key={m.id} model={m} counts={counts} pulseTrigger={pulseTrigger} />
          ))}
        </div>
      )}
    </div>
  );
}

function ModelPanel({ model, counts, pulseTrigger }: { model: any; counts: Record<string, number>; pulseTrigger: number }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [paths, setPaths] = useState<string[]>([]);

  // Build channel/endpoint data
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

  // Redraw SVG connectors
  const redraw = useCallback(() => {
    if (!containerRef.current) return;
    const box = containerRef.current.getBoundingClientRect();

    const modelEl = containerRef.current.querySelector('.snk-model') as HTMLElement;
    if (!modelEl) return;

    const mr = modelEl.getBoundingClientRect();
    const p0 = { x: mr.right - box.left, y: mr.top + mr.height / 2 - box.top };

    const result: string[] = [];
    chData.forEach((ch: any) => {
      const chEl = containerRef.current?.querySelector(`[data-n="${ch.key}"]`) as HTMLElement;
      if (!chEl) return;
      const cr = chEl.getBoundingClientRect();
      const p1 = { x: cr.left - box.left, y: cr.top + cr.height / 2 - box.top };
      const p1r = { x: cr.right - box.left, y: cr.top + cr.height / 2 - box.top };
      const mx = (p0.x + p1.x) / 2;
      result.push(`${p0.x},${p0.y},${mx},${p0.y},${mx},${p1.y},${p1.x},${p1.y}|${ch.key}`);

      ch.endpoints.forEach((ep: any) => {
        const epEl = containerRef.current?.querySelector(`[data-n="${ep.key}"]`) as HTMLElement;
        if (!epEl) return;
        const er = epEl.getBoundingClientRect();
        const p2 = { x: er.left - box.left, y: er.top + er.height / 2 - box.top };
        const mx2 = (p1r.x + p2.x) / 2;
        result.push(`${p1r.x},${p1r.y},${mx2},${p1r.y},${mx2},${p2.y},${p2.x},${p2.y}|${ep.key}`);
      });
    });
    setPaths(result);
  }, [chData]);

  useEffect(() => {
    redraw();
    window.addEventListener('resize', redraw);
    return () => window.removeEventListener('resize', redraw);
  }, [redraw, pulseTrigger]);

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
          {/* SVG connectors */}
          <svg className="absolute inset-0 w-full h-full pointer-events-none overflow-visible" style={{ zIndex: 0 }}>
            {paths.map((sp) => {
              const [coords] = sp.split('|');
              const pts = coords.split(',').map(Number);
              if (pts.length < 8) return null;
              return (
                <path
                  key={sp}
                  d={`M ${pts[0]} ${pts[1]} C ${pts[2]} ${pts[3]}, ${pts[4]} ${pts[5]}, ${pts[6]} ${pts[7]}`}
                  fill="none"
                  stroke="#d8d7d1"
                  strokeWidth="1.5"
                />
              );
            })}
          </svg>

          {/* Model column */}
          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">模型</div>
            <div className="snk-model border-2 border-primary rounded-lg px-3 py-2.5 bg-primary/5">
              <div className="text-sm font-semibold text-primary">{model.name}</div>
              <div className="text-[11px] text-muted-foreground">{model.model_pattern}</div>
            </div>
          </div>

          <div />

          {/* Channel column */}
          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">路由渠道</div>
            {chData.map((ch: any) => {
              const lv = loadLevel(ch.count, chCounts);
              return (
                <div key={ch.key} data-n={ch.key} className={`border rounded-lg px-3 py-2 transition-all ${LOAD_BORDER[lv]} ${LOAD_BG[lv]}`}>
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

          {/* Endpoint column */}
          <div className="flex flex-col gap-2 z-10">
            <div className="text-[10.5px] text-muted-foreground uppercase tracking-wider mb-1">渠道端点</div>
            {chData.map((ch: any) => {
              const epCounts = ch.endpoints.map((e: any) => e.count);
              return ch.endpoints.map((ep: any) => {
                const lv = loadLevel(ep.count, epCounts);
                return (
                  <div key={ep.key} data-n={ep.key} className={`border rounded-lg px-3 py-2 transition-all ${LOAD_BORDER[lv]} ${LOAD_BG[lv]}`}>
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
