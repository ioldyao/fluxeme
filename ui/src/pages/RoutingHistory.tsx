import { useState, useEffect, useCallback, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useModels } from '@/api/models';
import { fetchRoutingHistory } from '@/api/routing';
import type { RoutingHistoryResponse } from '@/api/routing';
import {
  BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip as ReTooltip, ResponsiveContainer,
  LineChart, Line,
} from 'recharts';

/**
 * ============================================================================
 * 历史负载查询面板（独立页面）
 * ============================================================================
 */

const C = {
  bg: '#f5f5f3',
  cardBg: '#ffffff',
  border: '#e4e3de',
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

const HISTORY_COLORS = ['#4a7fc9', '#d99a2b', '#6a4ec9', '#c94a4a', '#3ca07a', '#c47a3c'];

function rateClass(rate: number) {
  if (rate >= 97) return 'ok';
  if (rate >= 90) return 'warn';
  return 'bad';
}
const RATE_STYLE: Record<string, { color: string; bg: string }> = {
  ok: { color: C.green, bg: '#e7f5ea' },
  warn: { color: '#b4770a', bg: '#fbf1de' },
  bad: { color: '#c23b3b', bg: '#fbeaea' },
};

function formatBucket(bucket: string): string {
  if (bucket.includes('T')) return bucket.split('T')[1]?.slice(0, 5) || bucket;
  return bucket.slice(5);
}

export default function RoutingHistory() {
  const { t } = useTranslation();
  const { data: models } = useModels();
  const modelList = models || [];

  const [preset, setPreset] = useState('24h');
  const [customStart, setCustomStart] = useState('');
  const [customEnd, setCustomEnd] = useState('');
  const [modelFilter, setModelFilter] = useState('all');
  const [data, setData] = useState<RoutingHistoryResponse | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchData = useCallback(async (start: string, end: string) => {
    setLoading(true);
    try {
      const res = await fetchRoutingHistory(start, end, modelFilter !== 'all' ? modelFilter : undefined);
      setData(res);
    } catch {
      setData(null);
    } finally {
      setLoading(false);
    }
  }, [modelFilter]);

  useEffect(() => {
    const now = new Date();
    let start: string;
    const end = now.toISOString().slice(0, 16);
    switch (preset) {
      case '1h': { const d = new Date(now.getTime() - 3600000); start = d.toISOString().slice(0, 16); break; }
      case '24h': { const d = new Date(now.getTime() - 86400000); start = d.toISOString().slice(0, 16); break; }
      case '7d': { const d = new Date(now.getTime() - 7 * 86400000); start = d.toISOString().slice(0, 16); break; }
      case '30d': { const d = new Date(now.getTime() - 30 * 86400000); start = d.toISOString().slice(0, 16); break; }
      default: return;
    }
    fetchData(start.replace('T', ' ') + ':00', end.replace('T', ' ') + ':00');
  }, [preset, fetchData]);

  const handleApply = () => {
    if (!customStart || !customEnd) return;
    setPreset('');
    fetchData(customStart.replace('T', ' ') + ':00', customEnd.replace('T', ' ') + ':00');
  };

  const rangeLabel = preset
    ? ({ '1h': t('routingFlow.history1h'), '24h': t('routingFlow.history24h'), '7d': t('routingFlow.history7d'), '30d': t('routingFlow.history30d') } as Record<string, string>)[preset]
    : `${customStart.replace('T', ' ')} ~ ${customEnd.replace('T', ' ')}`;

  const volumeData = useMemo(() => {
    if (!data) return [];
    return data.buckets.map((bk, i) => {
      const row: Record<string, string | number> = { bucket: formatBucket(bk) };
      for (const [chId, s] of Object.entries(data.series)) row[chId] = s.volume[i] || 0;
      return row;
    });
  }, [data]);

  const successData = useMemo(() => {
    if (!data) return [];
    return data.buckets.map((bk, i) => {
      const row: Record<string, string | number> = { bucket: formatBucket(bk) };
      for (const [chId, s] of Object.entries(data.series)) row[chId] = s.success_rate[i] || 0;
      return row;
    });
  }, [data]);

  const channelIds = data ? Object.keys(data.series) : [];
  const totalReq = data ? data.summary.reduce((s, c) => s + c.requests, 0) : 0;

  const btnStyle = (p: string): React.CSSProperties => ({
    fontSize: 12.5, padding: '6px 12px', borderRadius: 6,
    border: `1px solid ${C.border}`,
    background: preset === p ? C.low : '#fafaf8',
    color: preset === p ? '#fff' : C.textSecondary,
    fontWeight: preset === p ? 500 : 400,
    cursor: 'pointer', transition: 'all 0.12s',
  });

  const fontFamily = '-apple-system, PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

  return (
    <div style={{ fontFamily, color: C.textPrimary }}>
      <h1 style={{ fontSize: 20, fontWeight: 600, margin: '0 0 4px' }}>{t('routingFlow.historyTitle')}</h1>
      <p style={{ fontSize: 13, color: C.textSecondary, margin: '0 0 20px' }}>{t('routingFlow.historySubtitle')}</p>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap', background: C.cardBg, border: `1px solid ${C.border}`, borderRadius: 10, padding: '12px 16px', marginBottom: 20 }}>
        <button style={btnStyle('1h')} onClick={() => setPreset('1h')}>{t('routingFlow.history1h')}</button>
        <button style={btnStyle('24h')} onClick={() => setPreset('24h')}>{t('routingFlow.history24h')}</button>
        <button style={btnStyle('7d')} onClick={() => setPreset('7d')}>{t('routingFlow.history7d')}</button>
        <button style={btnStyle('30d')} onClick={() => setPreset('30d')}>{t('routingFlow.history30d')}</button>
        <div style={{ width: 1, height: 20, background: C.border, margin: '0 4px' }} />
        <input type="datetime-local" style={{ fontSize: 12.5, padding: '6px 10px', borderRadius: 6, border: `1px solid ${C.border}`, color: C.textPrimary, background: '#fff' }} value={customStart} onChange={(e) => setCustomStart(e.target.value)} />
        <span style={{ color: C.textMuted, fontSize: 12 }}>{t('routingFlow.historyTo')}</span>
        <input type="datetime-local" style={{ fontSize: 12.5, padding: '6px 10px', borderRadius: 6, border: `1px solid ${C.border}`, color: C.textPrimary, background: '#fff' }} value={customEnd} onChange={(e) => setCustomEnd(e.target.value)} />
        <button style={{ fontSize: 12.5, padding: '6px 14px', borderRadius: 6, border: 'none', background: C.textPrimary, color: '#fff', cursor: 'pointer' }} onClick={handleApply}>{t('routingFlow.historyApply')}</button>
        <select style={{ fontSize: 12.5, padding: '6px 10px', borderRadius: 6, border: `1px solid ${C.border}`, color: C.textPrimary, background: '#fff', marginLeft: 'auto' }} value={modelFilter} onChange={(e) => setModelFilter(e.target.value)}>
          <option value="all">{t('routingFlow.historyAllModels')}</option>
          {modelList.map((m) => <option key={m.id} value={m.name}>{m.name}</option>)}
        </select>
      </div>

      {loading ? (
        <div style={{ fontSize: 13, color: C.textSecondary, padding: 40, textAlign: 'center' }}>{t('common.loading')}</div>
      ) : !data ? (
        <div style={{ borderRadius: 10, border: `1px dashed ${C.border}`, background: C.cardBg, padding: '40px 24px', textAlign: 'center', fontSize: 13, color: C.textSecondary }}>{t('routingFlow.noData')}</div>
      ) : (
        <>
          <div style={{ background: C.cardBg, border: `1px solid ${C.border}`, borderRadius: 10, padding: '18px 20px', marginBottom: 16 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
              <span style={{ fontSize: 13.5, fontWeight: 600 }}>{t('routingFlow.historyVolume')}</span>
              <span style={{ fontSize: 11.5, color: C.textMuted }}>{rangeLabel}</span>
            </div>
            <div style={{ width: '100%', height: 230 }}>
              <ResponsiveContainer>
                <BarChart data={volumeData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e1e0d9" />
                  <XAxis dataKey="bucket" tick={{ fill: '#898781', fontSize: 11 }} />
                  <YAxis tick={{ fill: '#898781', fontSize: 11 }} />
                  <ReTooltip contentStyle={{ borderRadius: 8, border: `1px solid ${C.border}`, fontSize: 12 }} />
                  {channelIds.map((chId, i) => (
                    <Bar key={chId} dataKey={chId} stackId="a" fill={HISTORY_COLORS[i % HISTORY_COLORS.length]} radius={[2, 2, 0, 0]} />
                  ))}
                </BarChart>
              </ResponsiveContainer>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 14, marginTop: 10, fontSize: 11.5, color: C.textSecondary }}>
              {channelIds.map((chId, i) => (
                <div key={chId} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <span style={{ width: 10, height: 10, borderRadius: 2, background: HISTORY_COLORS[i % HISTORY_COLORS.length], display: 'inline-block' }} />
                  {data!.series[chId]?.channel_name || chId}
                </div>
              ))}
            </div>
          </div>

          <div style={{ background: C.cardBg, border: `1px solid ${C.border}`, borderRadius: 10, padding: '18px 20px', marginBottom: 16 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
              <span style={{ fontSize: 13.5, fontWeight: 600 }}>{t('routingFlow.historySuccess')}</span>
            </div>
            <div style={{ width: '100%', height: 230 }}>
              <ResponsiveContainer>
                <LineChart data={successData}>
                  <CartesianGrid strokeDasharray="3 3" stroke="#e1e0d9" />
                  <XAxis dataKey="bucket" tick={{ fill: '#898781', fontSize: 11 }} />
                  <YAxis domain={[0, 100]} tick={{ fill: '#898781', fontSize: 11 }} tickFormatter={(v: number) => v + '%'} />
                  <ReTooltip contentStyle={{ borderRadius: 8, border: `1px solid ${C.border}`, fontSize: 12 }} formatter={(v: number) => [`${v}%`, '']} />
                  {channelIds.map((chId, i) => (
                    <Line key={chId} type="monotone" dataKey={chId} stroke={HISTORY_COLORS[i % HISTORY_COLORS.length]} strokeWidth={2} dot={false} />
                  ))}
                </LineChart>
              </ResponsiveContainer>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 14, marginTop: 10, fontSize: 11.5, color: C.textSecondary }}>
              {channelIds.map((chId, i) => (
                <div key={chId} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
                  <span style={{ width: 10, height: 10, borderRadius: 2, background: HISTORY_COLORS[i % HISTORY_COLORS.length], display: 'inline-block' }} />
                  {data!.series[chId]?.channel_name || chId}
                </div>
              ))}
            </div>
          </div>

          <div style={{ background: C.cardBg, border: `1px solid ${C.border}`, borderRadius: 10, overflow: 'hidden' }}>
            <table style={{ width: '100%', borderCollapse: 'collapse' }}>
              <thead>
                <tr>
                  <th style={{ textAlign: 'left', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableChannel')}</th>
                  <th style={{ textAlign: 'left', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableReqShare')}</th>
                  <th style={{ textAlign: 'right', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableRequests')}</th>
                  <th style={{ textAlign: 'right', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableSuccess')}</th>
                  <th style={{ textAlign: 'right', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableAvgLatency')}</th>
                  <th style={{ textAlign: 'right', fontSize: 11, fontWeight: 500, color: C.textMuted, textTransform: 'uppercase', letterSpacing: '0.03em', padding: '10px 18px', borderBottom: `1px solid ${C.border}`, background: C.nodeBg }}>{t('routingFlow.tableP95')}</th>
                </tr>
              </thead>
              <tbody>
                {data!.summary.map((s) => {
                  const pct = totalReq > 0 ? Math.round((s.requests / totalReq) * 100) : 0;
                  const barColor = pct >= 66 ? C.high : pct >= 33 ? C.mid : C.low;
                  const rs = RATE_STYLE[rateClass(s.success_rate)];
                  return (
                    <tr key={s.channel_id} style={{ fontWeight: 600, borderBottom: `1px solid ${C.border}` }}>
                      <td style={{ padding: '11px 18px', fontSize: 13, verticalAlign: 'middle' }}>{s.channel_id}</td>
                      <td style={{ padding: '11px 18px', fontSize: 13, verticalAlign: 'middle', minWidth: 140 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                          <div style={{ flex: 1, height: 6, background: C.barTrack, borderRadius: 3, overflow: 'hidden' }}>
                            <div style={{ height: '100%', borderRadius: 3, width: `${pct}%`, background: barColor }} />
                          </div>
                          <span style={{ fontSize: 12, color: C.textSecondary, minWidth: 34, textAlign: 'right' }}>{pct}%</span>
                        </div>
                      </td>
                      <td style={{ padding: '11px 18px', fontSize: 13, textAlign: 'right', verticalAlign: 'middle' }}>{s.requests.toLocaleString()}</td>
                      <td style={{ padding: '11px 18px', fontSize: 13, textAlign: 'right', verticalAlign: 'middle' }}>
                        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 12.5, fontWeight: 500, padding: '3px 9px', borderRadius: 6, color: rs.color, background: rs.bg }}>
                          <span style={{ width: 6, height: 6, borderRadius: '50%', background: rs.color }} />
                          {s.success_rate}%
                        </span>
                      </td>
                      <td style={{ padding: '11px 18px', fontSize: 13, textAlign: 'right', verticalAlign: 'middle' }}>{s.avg_latency}ms</td>
                      <td style={{ padding: '11px 18px', fontSize: 13, textAlign: 'right', verticalAlign: 'middle' }}>{s.p95_latency}ms</td>
                    </tr>
                  );
                })}
                {data!.summary.length === 0 && (
                  <tr><td colSpan={6} style={{ padding: 30, textAlign: 'center', fontSize: 13, color: C.textMuted }}>{t('routingFlow.tableEmpty')}</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}
