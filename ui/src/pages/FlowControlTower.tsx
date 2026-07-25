import { useState, useMemo, useEffect } from 'react';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useUsageFunnel, useUsageAggregate, useUsage, useModelActivity } from '@/api/usage';
import { useWalletOverview, useEstimatedDays } from '@/api/wallet';

// ── dark theme tokens matching gateway-flow-tower.html ──────────
const C = {
  ink: '#0A1210',
  ink2: '#0D1815',
  paper: '#E9F2EE',
  line: 'rgba(233,242,238,0.08)',
  lineSoft: 'rgba(233,242,238,0.04)',
  jade: '#4ADE80',
  jadeDim: '#1F5C3E',
  water: '#38BDF8',
  waterDim: '#145169',
  amber: '#FBBF24',
  rose: '#FB6F6F',
  inkText: '#E9F2EE',
  inkDim: '#7FA396',
  inkFaint: '#43584F',
  fontDisplay: '"Space Grotesk", ui-sans-serif, system-ui, sans-serif',
  fontBody: '"Inter", ui-sans-serif, system-ui, sans-serif',
  fontMono: '"JetBrains Mono", ui-monospace, monospace',
};

// ── helpers ─────────────────────────────────────────────────────
function fmtLatShort(ms: number) {
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

// ── Row for spill log ───────────────────────────────────────────
interface SpillRow {
  time: string;
  desc: string;
  kind: 'limit' | 'error' | 'upstream';
  dur: string;
  tag: string;
}

// ── River SVG component ─────────────────────────────────────────
function RiverFlowSVG({ modelShare, funnel }: {
  modelShare: { model: string; count: number; percentage: number }[];
  funnel: { total: number; successCount: number; rateLimitCount: number; upstreamErrCount: number; otherErrCount: number };
}) {
  if (!modelShare.length) return null;

  const maxCount = Math.max(...modelShare.map(m => m.count));
  const topN = modelShare.slice(0, 5);
  const vw = 1488; const vh = 380;

  return (
    <svg viewBox={`0 0 ${vw} ${vh}`} xmlns="http://www.w3.org/2000/svg" className="w-full h-auto block">
      <defs>
        <linearGradient id="fJade" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="#4ADE80" stopOpacity="0.85" />
          <stop offset="100%" stopColor="#4ADE80" stopOpacity="0.55" />
        </linearGradient>
        <linearGradient id="fWater" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="#4ADE80" stopOpacity="0.6" />
          <stop offset="100%" stopColor="#38BDF8" stopOpacity="0.75" />
        </linearGradient>
        <linearGradient id="fAmber" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="#4ADE80" stopOpacity="0.55" />
          <stop offset="100%" stopColor="#FBBF24" stopOpacity="0.7" />
        </linearGradient>
        <radialGradient id="gateGlow" cx="50%" cy="50%" r="50%">
          <stop offset="0%" stopColor="#4ADE80" stopOpacity="0.18" />
          <stop offset="100%" stopColor="#4ADE80" stopOpacity="0" />
        </radialGradient>
      </defs>

      {/* faint contour lines */}
      <g opacity="0.5">
        <path d="M0,50 C300,45 700,55 1488,48" stroke="#E9F2EE" strokeOpacity="0.03" strokeWidth="1" fill="none" />
        <path d="M0,330 C300,335 700,325 1488,332" stroke="#E9F2EE" strokeOpacity="0.03" strokeWidth="1" fill="none" />
      </g>

      {/* gate glow */}
      <ellipse cx="744" cy="190" rx="130" ry="180" fill="url(#gateGlow)" />

      {/* model streams → gate — dynamic from data */}
      {topN.map((m, i) => {
        const ratio = maxCount > 0 ? m.count / maxCount : 0.3;
        const w = Math.max(4, Math.min(28, 8 + ratio * 24));
        const y = 60 + i * 55;
        const isWarn = i === 1 || i === 3;
        const grad = isWarn ? 'url(#fAmber)' : 'url(#fJade)';
        return (
          <g key={m.model}>
            <path d={`M0,${y} C220,${y + 2} 420,${y + 40} 620,${130 + i * 10} C660,${145 + i * 8} 690,${155 + i * 6} 704,${170 + i * 5}`}
              fill="none" stroke={grad} strokeWidth={w} strokeLinecap="round" opacity={0.85} />
            {isWarn && (
              <>
                <path d={`M706,${172 + i * 5} C714,${173 + i * 5} 718,${174 + i * 5} 722,${175 + i * 5}`}
                  fill="none" stroke="#FB6F6F" strokeWidth={w * 0.6} strokeLinecap="round" opacity={0.9}
                  strokeDasharray="2 8" />
                <path d={`M724,${176 + i * 5} C730,${177 + i * 5} 736,${182 + i * 3} 744,190`}
                  fill="none" stroke={grad} strokeWidth={Math.max(4, w * 0.5)} strokeLinecap="round" />
              </>
            )}
          </g>
        );
      })}

      {/* gate node */}
      <circle cx="744" cy="190" r="42" fill="#0D1815" stroke="#4ADE80" strokeWidth="1.5" opacity="0.9" />
      <circle cx="744" cy="190" r="42" fill="none" stroke="#4ADE80" strokeWidth="1" opacity="0.35">
        <animate attributeName="r" values="42;58;42" dur="3s" repeatCount="indefinite" />
        <animate attributeName="opacity" values="0.35;0;0.35" dur="3s" repeatCount="indefinite" />
      </circle>

      {/* gate → upstream out */}
      <path d="M744,190 C820,190 900,165 1020,130 C1200,85 1350,70 1488,60"
        fill="none" stroke="url(#fJade)" strokeWidth={22} strokeLinecap="round" />
      <path d="M744,190 C820,192 900,220 1020,250 C1200,290 1350,305 1488,310"
        fill="none" stroke="url(#fWater)" strokeWidth={8} strokeLinecap="round" />

      {/* axis labels */}
      <text x="6" y="22" fill="#43584F" fontFamily="JetBrains Mono" fontSize="11" letterSpacing="1.5">MODELS · IN</text>
      <text x="670" y="22" fill="#43584F" fontFamily="JetBrains Mono" fontSize="11" letterSpacing="1.5">GATEWAY</text>
      <text x="1370" y="22" fill="#43584F" fontFamily="JetBrains Mono" fontSize="11" letterSpacing="1.5" textAnchor="end">UPSTREAM · OUT</text>
    </svg>
  );
}

// ── Clock component ─────────────────────────────────────────────
function Clock() {
  const [time, setTime] = useState(new Date());
  useEffect(() => {
    const id = setInterval(() => setTime(new Date()), 1000);
    return () => clearInterval(id);
  }, []);
  const p = (n: number) => String(n).padStart(2, '0');
  return <span>{p(time.getHours())}:{p(time.getMinutes())}:{p(time.getSeconds())}</span>;
}

// ── Main page ───────────────────────────────────────────────────
export default function FlowControlTower() {
  const [days] = useState(1);

  const { data: stats } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: funnel } = useUsageFunnel(days);
  const { data: ua } = useUsageAggregate(days);
  const { data: recent } = useUsage({ limit: 20 });
  const { data: ma } = useModelActivity(days);
  const { data: wo } = useWalletOverview();
  const { data: ed } = useEstimatedDays();

  // ── derived data ──────────────────────────────────────────────
  const availability = agg?.success_rate_24h ?? 0;
  const avgLat = agg?.avg_latency_ms_24h ?? 0;
  const requests24h = agg?.requests_24h ?? 0;
  const totalTokens24h = agg?.total_tokens_24h ?? 0;
  const modelCount = stats?.models ?? 0;
  const channelCount = stats?.channels ?? 0;
  const apiKeyCount = stats?.api_keys ?? 0;

  // model share
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

  // funnel data
  const funnelSafe = useMemo(() => {
    const total = funnel?.total ?? requests24h;
    const successCount = funnel?.success_count ?? Math.round(total * (availability / 100));
    const authCount = funnel?.auth_fail_count ?? 0;
    const rateLimitCount = funnel?.rate_limit_count ?? 0;
    const badReqCount = funnel?.bad_request_count ?? 0;
    const upstreamErrCount = funnel?.upstream_error_count ?? 0;
    const timeoutCount = funnel?.timeout_count ?? 0;
    const otherErrCount = funnel?.other_error_count ?? 0;
    return { total, successCount, authCount, rateLimitCount, badReqCount, upstreamErrCount, timeoutCount, otherErrCount };
  }, [funnel, requests24h, availability]);

  // spill log (recent errors/rate limits)
  const spills: SpillRow[] = useMemo(() => {
    if (!recent?.records) return [];
    const failed = recent.records.filter(r => !r.success).slice(0, 10);
    return failed.map(r => {
      const time = new Date(r.timestamp);
      const ts = `${String(time.getHours()).padStart(2, '0')}:${String(time.getMinutes()).padStart(2, '0')}:${String(time.getSeconds()).padStart(2, '0')}`;
      const sc = r.status_code;
      let kind: SpillRow['kind'] = 'error';
      let tag = '错误';
      if (sc === 429 || sc === 402) { kind = 'limit'; tag = '限流'; }
      else if (sc === 401 || sc === 403) { kind = 'limit'; tag = '鉴权'; }
      else if (sc >= 500) { kind = 'upstream'; tag = '上游'; }
      return {
        time: ts,
        desc: `${r.model} · ${sc} ${r.response_body ? r.response_body.slice(0, 40).replace(/\n/g, ' ') : ''}`,
        kind,
        dur: `${r.latency_ms}ms`,
        tag,
      };
    });
  }, [recent]);

  // timeline data (simplified)
  const timelinePoints = useMemo(() => {
    if (!ua?.length) return [];
    const maxCount = Math.max(...ua.map(d => d.count), 1);
    return ua.map((d, i) => ({
      x: (i / Math.max(1, ua.length - 1)) * 100,
      y: 100 - (d.count / maxCount) * 90,
      tokenY: 100 - (Math.min(d.total_tokens / Math.max(1, maxCount * 1000), 1)) * 70,
      count: d.count,
    }));
  }, [ua]);

  const funnelTotal = funnelSafe.total;
  const errorRate = funnelTotal > 0 ? ((funnelTotal - funnelSafe.successCount) / funnelTotal) * 100 : 0;
  const blocked = funnelSafe.authCount + funnelSafe.rateLimitCount;
  const upstreamErrTotal = funnelSafe.upstreamErrCount + funnelSafe.timeoutCount;
  const p99 = funnel?.p99_latency ?? avgLat;
  const p50 = funnel?.p50_latency ?? avgLat;
  const p95 = funnel?.p95_latency ?? avgLat;

  return (
    <div style={{ background: C.ink, color: C.inkText, fontFamily: C.fontBody, minHeight: '100vh' }}>
      <div style={{ maxWidth: 1560, margin: '0 auto', padding: '0 0 60px' }}>

        {/* ═══ STATUS BAR ═══ */}
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          padding: '20px 36px', borderBottom: `1px solid ${C.line}`,
          fontFamily: C.fontMono, fontSize: 12.5,
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 22 }}>
            <div style={{
              fontFamily: C.fontDisplay, fontSize: 17, fontWeight: 600, letterSpacing: 0.2,
              display: 'flex', alignItems: 'center', gap: 9, color: C.inkText,
            }}>
              <span style={{
                width: 8, height: 8, borderRadius: '50%', background: C.jade,
                boxShadow: `0 0 10px ${C.jade}`, display: 'inline-block',
              }} />
              流控台
            </div>
            <div style={{ color: C.inkDim, display: 'flex', alignItems: 'center', gap: 6 }}>
              网关运行 <b style={{ color: C.inkText, fontWeight: 600 }}>{availability >= 99 ? '稳定' : availability >= 95 ? '降级' : '异常'}</b>
            </div>
            <div style={{ color: C.inkDim, display: 'flex', alignItems: 'center', gap: 6 }}>
              模型 <b style={{ color: C.inkText, fontWeight: 600 }}>{modelCount}</b>
            </div>
            <div style={{ color: C.inkDim, display: 'flex', alignItems: 'center', gap: 6 }}>
              渠道 <b style={{ color: C.inkText, fontWeight: 600 }}>{channelCount}</b>
            </div>
            <div style={{
              color: C.inkDim, display: 'flex', alignItems: 'center', gap: 6,
            }}>
              成功率 <b style={{ color: errorRate > 5 ? C.rose : C.inkText, fontWeight: 600 }}>{availability.toFixed(2)}%</b>
            </div>
            <div style={{ color: C.inkDim, display: 'flex', alignItems: 'center', gap: 6 }}>
              刷新 <b style={{ color: C.inkText, fontWeight: 600 }}><Clock /></b>
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{
              border: `1px solid ${C.line}`, color: C.inkDim, background: 'transparent',
              fontFamily: C.fontMono, fontSize: 11.5, padding: '6px 12px', borderRadius: 20, cursor: 'default',
            }}>近 24 小时</span>
            <span style={{
              border: `1px solid ${C.line}`, color: C.amber, background: 'transparent',
              fontFamily: C.fontMono, fontSize: 11.5, padding: '6px 12px', borderRadius: 20, cursor: 'default',
              borderColor: 'rgba(251,191,36,0.3)',
            }}>⚠ 预警 · {funnelSafe.otherErrCount + blocked + upstreamErrTotal}</span>
          </div>
        </div>

        {/* ═══ HERO RIVER ═══ */}
        <div style={{ padding: '44px 36px 20px', position: 'relative' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-end', marginBottom: 6 }}>
            <h2 style={{
              fontFamily: C.fontDisplay, fontSize: 15, fontWeight: 600, letterSpacing: 2,
              textTransform: 'uppercase', color: C.inkDim,
            }}>实时流向 · 模型 → 网关 → 供应商</h2>
            <span style={{ fontFamily: C.fontMono, fontSize: 11, color: C.inkFaint }}>粗细 = 相对流量 · 断流处 = 被拦截</span>
          </div>

          <div style={{ position: 'relative', width: '100%' }}>
            <RiverFlowSVG modelShare={modelShare} funnel={funnelSafe} />

            {/* labels overlay */}
            <div style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
              {/* model tags on left */}
              {modelShare.slice(0, 5).map((m, i) => {
                const top = 9.5 + i * 18.5;
                const isWarn = i === 1 || i === 3;
                const isBad = !isWarn && m.percentage < 5;
                return (
                  <div key={m.model} style={{
                    position: 'absolute', left: 0, top: `${top}%`,
                    fontFamily: C.fontMono, fontSize: 11.5, color: C.inkText,
                    display: 'flex', alignItems: 'center', gap: 8, whiteSpace: 'nowrap',
                  }}>
                    <span style={{ fontWeight: 600 }}>{m.model.length > 12 ? `${m.model.slice(0, 10)}..` : m.model}</span>
                    <span style={{ color: C.inkFaint, fontSize: 10 }}>{fmtCount(m.count)}</span>
                    <span style={{
                      fontSize: 9.5, padding: '1px 6px', borderRadius: 8,
                      background: isWarn ? 'rgba(251,191,36,0.15)' : isBad ? 'rgba(251,111,111,0.15)' : 'rgba(74,222,128,0.12)',
                      color: isWarn ? C.amber : isBad ? C.rose : C.jade,
                    }}>{isWarn ? '高负载' : isBad ? '低流量' : '平稳'}</span>
                  </div>
                );
              })}

              {/* gate readout */}
              <div style={{
                position: 'absolute', left: '50%', top: '36%', transform: 'translate(-50%, -50%)',
                fontFamily: C.fontMono, textAlign: 'center',
              }}>
                <div style={{ fontFamily: C.fontDisplay, fontSize: 30, fontWeight: 700, color: C.inkText }}>
                  {blocked + upstreamErrTotal}
                </div>
                <div style={{ fontSize: 10, color: C.inkFaint, letterSpacing: 1, textTransform: 'uppercase', marginTop: 2 }}>
                  拦截 / 分钟
                </div>
                <div style={{ fontSize: 10.5, color: C.rose, marginTop: 6 }}>
                  限流 {funnelSafe.rateLimitCount} · 错误 {funnelSafe.upstreamErrCount + funnelSafe.otherErrCount}
                </div>
              </div>

              {/* upstream tags */}
              <div style={{
                position: 'absolute', right: 0, top: '12%',
                fontFamily: C.fontMono, fontSize: 12, color: C.inkText, textAlign: 'right',
              }}>
                <span style={{ fontWeight: 600 }}>成功</span> · <span style={{ fontSize: 10, color: C.inkFaint }}>{fmtCount(funnelSafe.successCount)} req</span>
              </div>
              <div style={{
                position: 'absolute', right: 0, top: '68%',
                fontFamily: C.fontMono, fontSize: 12, color: C.inkText, textAlign: 'right',
              }}>
                <span style={{ fontWeight: 600 }}>失败/拦截</span> · <span style={{ fontSize: 10, color: C.inkFaint }}>{fmtCount(funnelTotal - funnelSafe.successCount)} req</span>
              </div>
            </div>
          </div>
        </div>

        {/* ═══ WATERLINE METRICS ═══ */}
        <div style={{ display: 'flex', gap: 0, margin: '6px 36px 0', borderTop: `1px solid ${C.line}`, borderBottom: `1px solid ${C.line}` }}>
          {[
            { k: '请求总量', v: fmtCount(funnelTotal), d: `${fmtTokens(totalTokens24h)} tokens · ${funnelTotal > 0 ? (funnelTotal / 86400).toFixed(1) : '0'} QPS`, cls: '' },
            { k: '请求时长 P99', v: fmtLatShort(p99), d: `P50 ${fmtLatShort(p50)} · P95 ${fmtLatShort(p95)}`, cls: funnelSafe.total > 0 && p99 > 5000 ? 'warn' : '' },
            { k: '成功率', v: `${availability.toFixed(2)}%`, d: `错误率 ${errorRate.toFixed(2)}%`, cls: errorRate > 5 ? 'bad' : errorRate > 2 ? 'warn' : 'good' },
            { k: '上游错误', v: String(upstreamErrTotal), d: `429: ${funnelSafe.rateLimitCount} · 5xx: ${funnelSafe.upstreamErrCount}`, cls: upstreamErrTotal > 0 ? 'warn' : 'good' },
            { k: '密钥 / 配额', v: String(apiKeyCount), d: ed?.days != null ? `可用 ${ed.days.toFixed(1)}d` : '—', cls: '' },
          ].map(m => (
            <div key={m.k} style={{ flex: 1, padding: '16px 22px', borderRight: `1px solid ${C.line}`, '--last-border': 'none' } as React.CSSProperties}>
              <div style={{ fontFamily: C.fontMono, fontSize: 10.5, color: C.inkFaint, letterSpacing: 1, textTransform: 'uppercase' }}>{m.k}</div>
              <div style={{
                fontFamily: C.fontDisplay, fontSize: 22, fontWeight: 700,
                color: m.cls === 'bad' ? C.rose : m.cls === 'warn' ? C.amber : m.cls === 'good' ? C.jade : C.inkText,
              }}>{m.v}</div>
              <div style={{ fontFamily: C.fontMono, fontSize: 10.5, color: C.inkDim }}>{m.d}</div>
            </div>
          ))}
        </div>

        {/* ═══ TIMELINE ═══ */}
        <div style={{ padding: '36px 36px 0' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', marginBottom: 14 }}>
            <h2 style={{
              fontFamily: C.fontDisplay, fontSize: 15, fontWeight: 600, letterSpacing: 2,
              textTransform: 'uppercase', color: C.inkDim,
            }}>24 小时回放</h2>
            <div style={{ display: 'flex', gap: 16, fontFamily: C.fontMono, fontSize: 11, color: C.inkDim }}>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ width: 10, height: 2, background: C.jade, display: 'inline-block' }} />QPS
              </span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ width: 10, height: 2, background: C.water, display: 'inline-block' }} />TPS（千）
              </span>
              <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                <span style={{ width: 10, height: 2, background: C.amber, display: 'inline-block' }} />错误事件
              </span>
            </div>
          </div>

          {/* scrub track */}
          <div style={{ position: 'relative', height: 120, borderBottom: `1px solid ${C.line}` }}>
            <svg viewBox="0 0 1488 120" preserveAspectRatio="none" style={{ width: '100%', height: '100%', display: 'block' }}>
              <polyline fill="none" stroke={C.jade} strokeWidth="1.8"
                points={timelinePoints.map(p => `${(p.x / 100) * 1488},${p.y}`).join(' ')} />
              <polyline fill="none" stroke={C.water} strokeWidth="1.4" opacity="0.75"
                points={timelinePoints.map(p => `${(p.x / 100) * 1488},${p.tokenY}`).join(' ')} />
              {/* error markers */}
              {ua && ua.map((d, i) => {
                const errCount = d.count - ((d as any).success_count ?? 0);
                if (errCount <= 0) return null;
                const x = (i / Math.max(1, ua.length - 1)) * 1488;
                const y = 100 - Math.min(errCount / 5, 1) * 90;
                return <circle key={i} cx={x} cy={y} r={2 + Math.min(errCount, 5)} fill={C.amber} opacity={0.8} />;
              })}
            </svg>
          </div>
          <div style={{
            display: 'flex', justifyContent: 'space-between', padding: '8px 0 0',
            fontFamily: C.fontMono, fontSize: 10, color: C.inkFaint,
          }}>
            {ua && ua.length > 0 && (
              <>
                <span>{ua[0]?.date}</span>
                <span>{ua[Math.max(0, Math.floor(ua.length / 3))]?.date}</span>
                <span>{ua[Math.max(0, Math.floor(ua.length * 2 / 3))]?.date}</span>
                <span>{ua[ua.length - 1]?.date}</span>
              </>
            )}
          </div>
        </div>

        {/* ═══ BOTTOM: spill log + gauges ═══ */}
        <div style={{
          display: 'grid', gridTemplateColumns: '1.3fr 1fr', gap: 0, marginTop: 36,
        }}>
          {/* spill log */}
          <div style={{ padding: '0 36px', borderRight: `1px solid ${C.line}` }}>
            <h3 style={{
              fontFamily: C.fontDisplay, fontSize: 14, fontWeight: 600, letterSpacing: 1.5,
              textTransform: 'uppercase', color: C.inkDim, marginBottom: 18,
            }}>溢出记录 · 最近拦截</h3>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              {spills.length === 0 ? (
                <div style={{ fontFamily: C.fontMono, fontSize: 12, color: C.inkFaint, padding: '20px 0', textAlign: 'center' }}>
                  暂无异常记录
                </div>
              ) : spills.slice(0, 8).map((s, i) => (
                <div key={i} style={{
                  display: 'grid', gridTemplateColumns: '90px 1fr 60px 60px', gap: 14, alignItems: 'center',
                  padding: '11px 0', borderBottom: `1px solid ${C.lineSoft}`, fontFamily: C.fontMono, fontSize: 12,
                }}>
                  <span style={{ color: C.inkFaint }}>{s.time}</span>
                  <span style={{ color: C.inkText, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    <span style={{ color: C.inkDim }}>{s.desc}</span>
                  </span>
                  <span style={{
                    fontSize: 10.5, padding: '2px 8px', borderRadius: 8, textAlign: 'center',
                    background: s.kind === 'limit' ? 'rgba(251,191,36,0.12)' :
                      s.kind === 'upstream' ? 'rgba(56,189,248,0.12)' : 'rgba(251,111,111,0.12)',
                    color: s.kind === 'limit' ? C.amber : s.kind === 'upstream' ? C.water : C.rose,
                  }}>{s.tag}</span>
                  <span style={{ textAlign: 'right', color: C.inkFaint }}>{s.dur}</span>
                </div>
              ))}
            </div>
          </div>

          {/* base water level */}
          <div style={{ padding: '0 36px' }}>
            <h3 style={{
              fontFamily: C.fontDisplay, fontSize: 14, fontWeight: 600, letterSpacing: 1.5,
              textTransform: 'uppercase', color: C.inkDim, marginBottom: 18,
            }}>网关水位</h3>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: '20px 26px' }}>
              {[
                { k: '请求数', v: fmtCount(requests24h), pct: Math.min(100, (requests24h / 100000) * 100), meta: `24h 总计` },
                { k: 'Token', v: fmtTokens(totalTokens24h), pct: Math.min(100, (totalTokens24h / 50000000) * 100), meta: `24h 总计` },
                { k: '成功率', v: `${availability.toFixed(1)}%`, pct: availability, meta: `${funnelSafe.successCount} / ${funnelTotal} 成功`, cls: availability < 90 ? C.rose : availability < 97 ? C.amber : C.jade },
                { k: '模型数', v: String(modelCount), pct: Math.min(100, modelCount * 8), meta: `已配置` },
                { k: '渠道', v: String(channelCount), pct: Math.min(100, channelCount * 12), meta: `已配置` },
                { k: '余额', v: wo?.balance != null ? `¥${wo.balance.toFixed(2)}` : '—', pct: Math.min(100, ((wo?.balance ?? 0) / 1000) * 100), meta: ed?.days != null ? `可用 ${ed.days.toFixed(1)}d` : '—' },
              ].map(g => (
                <div key={g.k} style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                  <div style={{ fontFamily: C.fontMono, fontSize: 10.5, color: C.inkFaint, letterSpacing: 1, textTransform: 'uppercase' }}>{g.k}</div>
                  <div style={{ fontFamily: C.fontDisplay, fontSize: 20, fontWeight: 700, color: (g as any).cls ?? C.inkText }}>{g.v}</div>
                  <div style={{ height: 3, background: C.line, borderRadius: 2, overflow: 'hidden' }}>
                    <div style={{ height: '100%', borderRadius: 2, background: (g as any).cls ?? C.jade, width: `${g.pct}%` }} />
                  </div>
                  <div style={{ fontFamily: C.fontMono, fontSize: 9.5, color: C.inkFaint }}>{g.meta}</div>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div style={{
          marginTop: 44, padding: '20px 36px 0', textAlign: 'center',
          fontFamily: C.fontMono, fontSize: 10.5, color: C.inkFaint, letterSpacing: 0.5,
          borderTop: `1px solid ${C.line}`,
        }}>
          数据每 60s 刷新一次 · 如数据异常请检查网关配置
        </div>

      </div>
    </div>
  );
}
