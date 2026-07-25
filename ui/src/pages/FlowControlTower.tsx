import { useState, useMemo, useEffect, useRef } from 'react';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useRoutingHistory } from '@/api/routing';
import { useUsageFunnel, useUsageAggregate, useUsage, useModelActivity } from '@/api/usage';
import { useWalletOverview, useEstimatedDays } from '@/api/wallet';

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

// ── River SVG: model streams through gateway → upstream ─────────
function RiverFlowSVG({
  modelShare,
  channelCount,
}: {
  modelShare: { model: string; count: number; percentage: number }[];
  channelCount: number;
}) {
  if (!modelShare.length) return null;

  const maxCount = Math.max(...modelShare.map(m => m.count));
  const topN = modelShare.slice(0, 5);
  // fill remaining slots with placeholders so we always have 5 bands
  const bands = topN.length >= 5 ? topN : [
    ...topN,
    ...Array.from({ length: 5 - topN.length }, (_, i) => ({
      model: `—`,
      count: 0,
      percentage: 0,
    })),
  ];

  return (
    <svg viewBox="0 0 1200 520" preserveAspectRatio="none" className="w-full h-full block">
      <defs>
        <linearGradient id="wm" x1="0" x2="1">
          <stop offset="0" stopColor="#77afa5" />
          <stop offset="1" stopColor="#267b7b" />
        </linearGradient>
        <linearGradient id="wl" x1="0" x2="1">
          <stop offset="0" stopColor="#9bc4ba" />
          <stop offset="1" stopColor="#4f9b8e" />
        </linearGradient>
      </defs>
      <g fill="none" strokeLinecap="round" opacity={0.9}>
        {/* 5 model streams converging to gate */}
        {bands.map((_, i) => {
          const ratio = maxCount > 0 ? (bands[i]?.count ?? 0) / maxCount : 0.3;
          const w = Math.max(8, Math.min(36, 10 + ratio * 30));
          const y = 76 + i * 90;
          const grad = i < 2 ? 'url(#wm)' : 'url(#wl)';
          return (
            <path
              key={`in-${i}`}
              d={`M145 ${y} C330 ${y} 375 ${205 + i * 8} 510 ${222 + i * 8}`}
              stroke={grad}
              strokeWidth={w}
            />
          );
        })}
        {/* error / rate-limit trickle */}
        <path
          d="M145 444 C270 444 345 370 430 344"
          stroke="#c65d50" strokeWidth="5" strokeDasharray="9 8"
        />
        {/* gate → upstream out (main + secondary) */}
        <path d="M690 238 C835 220 895 120 1055 105" stroke="url(#wm)" strokeWidth={54} />
        <path d="M690 262 C845 270 900 305 1055 316" stroke="url(#wl)" strokeWidth={channelCount > 1 ? 20 : 8} />
        {channelCount > 2 && (
          <path d="M690 282 C820 320 915 415 1055 432" stroke="#8bb9ae" strokeWidth={12} />
        )}
      </g>
      {/* annotation text */}
      {topN.length > 0 && (
        <g fontFamily="Inter, system-ui, sans-serif" fontSize="12" fill="#688082">
          <text x="400" y="341">× 业务限制 / 鉴权失败</text>
          <text x="410" y="365">× 系统错误 / 超时</text>
        </g>
      )}
    </svg>
  );
}

// ── Timeline scrub ──────────────────────────────────────────────
function TimelineScrub({ aggregates }: {
  aggregates: { date: string; count: number; total_tokens: number; success_count: number }[];
}) {
  const [pos, setPos] = useState(24);
  const maxCount = Math.max(...aggregates.map(d => d.count), 1);
  const factor = 0.55 + 0.45 * Math.sin((pos / 24) * Math.PI);

  const points = useMemo(() => {
    if (!aggregates.length) return '';
    return aggregates.map((d, i) => {
      const x = (i / Math.max(1, aggregates.length - 1)) * 100;
      const y = 100 - (d.count / maxCount) * 85;
      return `${(x / 100) * 1200},${y}`;
    }).join(' ');
  }, [aggregates, maxCount]);

  const timeLabel = pos === 24 ? '现在' : `${String(pos).padStart(2, '0')}:00 · 历史回放`;
  const peakTps = aggregates.length > 0
    ? Math.max(...aggregates.map(d =>
        Math.round(d.total_tokens / 86400 * (aggregates.length > 1 ? aggregates.length : 1))
      ))
    : 0;

  return (
    <div>
      <div className="flex justify-between text-[11px] text-muted-foreground mb-1.5">
        <span>24 小时流量回放</span>
        <span>{timeLabel}</span>
      </div>
      <input
        type="range" min={0} max={24} value={pos} step={1}
        onChange={e => setPos(Number(e.target.value))}
        className="w-full accent-[#267b7b]"
      />
      <div className="flex justify-between gap-3 text-[11px] text-muted-foreground mt-1">
        <span>00:00</span>
        <span>06:00</span>
        <span>12:00</span>
        <span>18:00</span>
        <span>现在</span>
      </div>
      <div className="flex gap-4 flex-wrap mt-3 text-[11px] text-muted-foreground">
        <span><i className="inline-block w-[22px] h-[9px] rounded-full mr-1 align-middle" style={{ background: '#267b7b' }} />主流量</span>
        <span><i className="inline-block w-[22px] h-[5px] rounded-full mr-1 align-middle" style={{ background: '#8bb9ae' }} />低流量</span>
        <span><i className="inline-block w-[22px] h-[5px] rounded-full mr-1 align-middle" style={{ background: '#c65d50' }} />拦截/错误</span>
        <span className="text-muted-foreground/60">河道宽度 = 请求量 · {fmtCount(Math.round(peakTps * factor))} TPS</span>
      </div>
    </div>
  );
}

// ── Main page ───────────────────────────────────────────────────
export default function FlowControlTower() {
  const [days] = useState(1);

  const { data: stats } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: funnel } = useUsageFunnel(days);
  const { data: ua } = useUsageAggregate(days);
  const { data: ma } = useModelActivity(days);
  const { data: rh } = useRoutingHistory(1, { enabled: true });
  const { data: wo } = useWalletOverview();
  const { data: ed } = useEstimatedDays();

  // ── derived ───────────────────────────────────────────────────
  const availability = agg?.success_rate_24h ?? 0;
  const avgLat = agg?.avg_latency_ms_24h ?? 0;
  const requests24h = agg?.requests_24h ?? 0;
  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const modelCount = stats?.models ?? 0;
  const channelCount = stats?.channels ?? 0;

  // model share (top 5 by request volume)
  const modelShare = useMemo(() => {
    if (!ma?.length) return [];
    const sorted = ma.slice().sort((a, b) => b.total_requests - a.total_requests);
    const total = sorted.reduce((s, i) => s + i.total_requests, 0);
    return sorted.slice(0, 5).map(i => ({
      model: i.model,
      count: i.total_requests,
      percentage: total > 0 ? (i.total_requests / total) * 100 : 0,
    }));
  }, [ma]);

  // funnel breakdown
  const funnelSafe = useMemo(() => {
    const total = funnel?.total ?? requests24h;
    const successCount = funnel?.success_count ?? Math.round(total * (availability / 100));
    const authCount = funnel?.auth_fail_count ?? 0;
    const rateLimitCount = funnel?.rate_limit_count ?? 0;
    const upstreamErrCount = funnel?.upstream_error_count ?? 0;
    const timeoutCount = funnel?.timeout_count ?? 0;
    const otherErrCount = funnel?.other_error_count ?? 0;
    return { total, successCount, authCount, rateLimitCount, upstreamErrCount, timeoutCount, otherErrCount };
  }, [funnel, requests24h, availability]);

  // channel/provider breakdown from routing history
  const providerBreakdown = useMemo(() => {
    if (!rh?.summary?.length) return [];
    const total = rh.summary.reduce((s, r) => s + r.requests, 0);
    return rh.summary
      .slice()
      .sort((a, b) => b.requests - a.requests)
      .slice(0, 3)
      .map(r => ({
        name: rh.series[r.channel_id]?.channel_name ?? r.channel_id,
        requests: r.requests,
        share: total > 0 ? (r.requests / total) * 100 : 0,
        p99: r.p95_latency,
      }));
  }, [rh]);

  const funnelTotal = funnelSafe.total;
  const errorRate = funnelTotal > 0 ? ((funnelTotal - funnelSafe.successCount) / funnelTotal) * 100 : 0;
  const blocked = funnelSafe.authCount + funnelSafe.rateLimitCount;
  const upstreamErrTotal = funnelSafe.upstreamErrCount + funnelSafe.timeoutCount;
  const p99 = funnel?.p99_latency ?? avgLat;
  const p50 = funnel?.p50_latency ?? avgLat;
  const p95 = funnel?.p95_latency ?? avgLat;
  const qps = funnelTotal > 0 ? (funnelTotal / 86400) : 0;
  const tps = totalTokens24h > 0 ? (totalTokens24h / 86400) : 0;

  return (
    <div className="space-y-0">
      {/* ═══ TOWER HEADER ═══ */}
      <div className="flex items-center justify-between gap-5 py-3 px-0.5 border-b text-sm flex-wrap">
        <div className="flex items-center gap-5 flex-wrap">
          <span className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-full bg-emerald-600 shadow-[0_0_0_0_rgba(5,150,105,0.35)] animate-pulse" />
            <b>塔台健康 {Math.round(availability)}</b>
          </span>
          <span className="text-muted-foreground">
            <strong className="text-foreground">{modelCount}</strong> 模型在线
          </span>
          <span className="text-muted-foreground">
            SLA <strong className={availability >= 99 ? 'text-emerald-700' : 'text-amber-700'}>{availability.toFixed(2)}%</strong>
          </span>
        </div>
        <div className="flex items-center gap-5 flex-wrap">
          <span className="text-muted-foreground">
            请求 <strong className="text-foreground tabular-nums">{fmtCount(funnelTotal)}</strong>
          </span>
          <span className="text-muted-foreground">
            Token <strong className="text-foreground tabular-nums">{fmtTokens(totalTokens24h)}</strong>
          </span>
          <span className="text-muted-foreground">
            TPS <strong className="text-foreground tabular-nums">{tps.toFixed(1)}</strong>
          </span>
        </div>
      </div>

      {/* ═══ RIVER SECTION ═══ */}
      <div className="py-5">
        {/* three-column labels */}
        <div className="grid grid-cols-3 text-xs text-muted-foreground mb-2">
          <div>模型入口</div>
          <div className="text-center">网关闸门</div>
          <div className="text-right">供应商出口</div>
        </div>

        {/* river visual */}
        <section className="relative h-[520px] border-y overflow-hidden">
          <RiverFlowSVG modelShare={modelShare} channelCount={channelCount} />

          {/* model list (left) */}
          <div className="absolute inset-y-[42px] left-0 flex flex-col justify-around z-[3]">
            {modelShare.slice(0, 5).map((m, i) => (
              <div key={m.model} className="min-w-[150px]">
                <b className="text-sm leading-tight">
                  {m.model.length > 14 ? `${m.model.slice(0, 12)}..` : m.model}
                </b>
                <span className="text-[11px] text-muted-foreground block">
                  {fmtCount(m.count)} req · {m.percentage.toFixed(0)}%
                </span>
              </div>
            ))}
            {modelShare.length === 0 && (
              <div className="text-xs text-muted-foreground">暂无模型数据</div>
            )}
          </div>

          {/* provider list (right) */}
          <div className="absolute inset-y-[42px] right-0 flex flex-col justify-around z-[3] text-right">
            {providerBreakdown.length > 0 ? providerBreakdown.map(p => (
              <div key={p.name} className="min-w-[150px]">
                <b className="text-sm leading-tight">{p.name}</b>
                <span className="text-[11px] text-muted-foreground block">
                  {fmtCount(p.requests)} req · {p.share.toFixed(1)}%
                </span>
              </div>
            )) : (
              <div className="text-xs text-muted-foreground">暂无数据</div>
            )}
          </div>

          {/* gate */}
          <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-[190px] h-[240px] z-[4]
            flex flex-col items-center justify-center gap-0.5
            border border-[rgba(38,123,123,0.4)] rounded-[48%_48%_24%_24%/18%_18%_30%_30%]
            bg-[rgba(236,246,244,0.86)] backdrop-blur shadow-[inset_0_0_0_8px_rgba(38,123,123,0.04)]">
            <span className="text-[11px] text-muted-foreground tracking-wider uppercase">GATEWAY</span>
            <h2 className="text-lg font-semibold my-1">流量闸门</h2>
            <div className="grid grid-cols-2 gap-1 w-[142px]">
              <div className="py-2 px-1.5 border-t text-center">
                <b className="text-lg tabular-nums">{upstreamErrTotal + otherErrCount}</b>
                <span className="block text-[10px] text-muted-foreground">异常拦截</span>
              </div>
              <div className="py-2 px-1.5 border-t text-center">
                <b className="text-lg tabular-nums">{blocked}</b>
                <span className="block text-[10px] text-muted-foreground">业务限制</span>
              </div>
              <div className="py-2 px-1.5 border-t text-center">
                <b className="text-lg tabular-nums">{availability.toFixed(1)}%</b>
                <span className="block text-[10px] text-muted-foreground">SLA</span>
              </div>
              <div className="py-2 px-1.5 border-t text-center">
                <b className="text-lg tabular-nums">{qps.toFixed(1)}</b>
                <span className="block text-[10px] text-muted-foreground">QPS</span>
              </div>
            </div>
          </div>

          {/* waterline */}
          <div className="absolute left-[25%] right-[25%] bottom-[23px] flex justify-around text-[11px] text-muted-foreground z-[5]">
            <span>TTFT P50 {fmtLat(p50)}</span>
            <span>TTFT P99 {fmtLat(p99)}</span>
            <span>请求 P95 {fmtLat(p95)}</span>
            <span>Max {fmtLat(funnel?.p99_latency ?? avgLat)}</span>
          </div>
        </section>
      </div>

      {/* ═══ TIMELINE ═══ */}
      <section className="pt-4">
        {ua && ua.length > 0 ? (
          <TimelineScrub aggregates={ua} />
        ) : (
          <div className="text-xs text-muted-foreground text-center py-8">暂无时序数据</div>
        )}
      </section>
    </div>
  );
}
