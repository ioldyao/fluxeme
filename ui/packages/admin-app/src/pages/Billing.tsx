import { useState, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';
import {
  BarChart, Bar, XAxis, YAxis, Tooltip, Legend, ResponsiveContainer, LineChart, Line, PieChart, Pie, Cell,
} from 'recharts';
import {
  useAdminBillingSummary, useAdminBillingUserSpendRanking, useAdminBillingDailyTrend, useAdminBillingMonths, useAdminScopedPeriodSummary, useAdminBillingUserApiKeyCosts, useAdminDeductions,
} from '@fluxeme/shared/src/api/billing';
import { api } from '@fluxeme/shared/src/api/client';
import { useQuery } from '@tanstack/react-query';

// ── helpers ───────────────────────────────────────

const fmtMoney = (n: number) => {
  const abs = Math.abs(n);
  if (abs === 0) return '¥0.00';
  if (abs < 0.01) return `¥${abs.toFixed(6)}`;
  return `¥${n < 0 ? '-' : ''}${abs.toFixed(2)}`;
};
const fmtShort = (n: number) => {
  if (n >= 1_000_000_000) return (n / 1_000_000_000).toFixed(2) + 'B';
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(2) + 'M';
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
  return String(n);
};

const now = () => { const d = new Date(); return { year: d.getFullYear(), month: d.getMonth() + 1 }; };
const { year: curYear, month: curMonth } = now();
const monthLabel = (y: number, m: number) => `${y} 年 ${m} 月${y === curYear && m === curMonth ? '（本期）' : ''}`;

const COLORS = ['var(--accent-foreground)', 'var(--chart-3)', 'var(--chart-2)', 'var(--sidebar-primary)'];

/** Fill an array with all days of {year, month}, copying known data
 *  from `source` (keyed by "MM-DD" label) and defaulting to 0 for
 *  missing days.  Dates after today are excluded. */
function fillMonthDays<T extends { label: string }>(
  source: T[],
  year: number,
  month: number,
  _field: 'cost' | 'cost_requests',
): (T & { cost: number; requests?: number })[] {
  const today = new Date();
  // Compare using numeric year+month+day to avoid string comparison pitfalls
  const todayYear = today.getFullYear();
  const todayMonth = today.getMonth() + 1;
  const todayDay = today.getDate();
  const lookup = new Map<string, T>();
  for (const item of source) lookup.set(item.label, item);

  const daysInMonth = new Date(year, month, 0).getDate();
  const result: (T & { cost: number; requests?: number })[] = [];
  for (let d = 1; d <= daysInMonth; d++) {
    const key = `${String(month).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
    // skip future dates — numeric comparison to avoid "8-18" > "08-18" string issues
    if (todayYear === year && todayMonth === month && d > todayDay) break;
    if (todayYear > year || (todayYear === year && todayMonth > month)) break;
    const exist = lookup.get(key);
    if (exist) {
      result.push(exist as any);
    } else {
      result.push({ label: key, cost: 0, requests: 0 } as any);
    }
  }
  return result;
}

// ── User Billing Overview ─────────────────────────

function UserBillingOverview({ onSelectUser }: { onSelectUser: (uid: string) => void }) {
  const [searchParams, setSearchParams] = useSearchParams();
  const year = parseInt(searchParams.get('year') ?? '', 10) || curYear;
  const month = parseInt(searchParams.get('month') ?? '', 10) || curMonth;
  const [search, setSearch] = useState('');
  const [sortBy, setSortBy] = useState('cost_desc');

  const { data: summary } = useAdminBillingSummary();
  const { data: ranking } = useAdminBillingUserSpendRanking(year, month, 100);
  const { data: trend } = useAdminBillingDailyTrend(year, month);
  const { data: months } = useAdminBillingMonths();

  const filtered = useMemo(() => {
    const items = ranking?.items ?? [];
    return items
      .filter((r) => !search || r.user_name.toLowerCase().includes(search.toLowerCase()) || r.user_id.toLowerCase().includes(search.toLowerCase()))
      .sort((a, b) => {
        if (sortBy === 'requests_desc') return b.total_requests - a.total_requests;
        if (sortBy === 'tokens_desc') return b.total_tokens - a.total_tokens;
        return b.total_cost - a.total_cost;
      });
  }, [ranking, search, sortBy]);

  const concentration = useMemo(() => {
    const items = ranking?.items ?? [];
    const sorted = [...items].sort((a, b) => b.total_cost - a.total_cost);
    const totalCost = sorted.reduce((s, i) => s + i.total_cost, 0);
    if (!totalCost) return null;
    const top10 = sorted.slice(0, 10);
    const top20 = sorted.slice(0, 20);
    const top10Cost = top10.reduce((s, i) => s + i.total_cost, 0);
    const top20Cost = top20.reduce((s, i) => s + i.total_cost, 0);
    const otherCost = Math.max(0, sorted.slice(20).reduce((s, i) => s + i.total_cost, 0));
    return {
      top10Cost, top10Pct: (top10Cost / totalCost) * 100,
      top20Cost, top20Pct: (top20Cost / totalCost) * 100,
      otherCost, otherPct: (otherCost / totalCost) * 100,
    };
  }, [ranking]);

  const trendData = useMemo(() => fillMonthDays(
    (trend ?? []).map((p) => ({ label: p.date.slice(5, 10), cost: p.total_cost })),
    year, month, 'cost',
  ), [trend, year, month]);

  const handleMonthChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const [y, m] = e.target.value.split('-').map(Number);
    setSearchParams({ year: String(y), month: String(m) });
  };

  const totalCost = summary?.total_cost ?? 0;
  const totalRequests = summary?.total_requests ?? 0;
  const totalTokens = summary?.total_tokens ?? 0;
  const activeUsers = ranking?.items.length ?? 0;
  const totalKeys = ranking?.items.reduce((s, i) => s + i.api_key_count, 0) ?? 0;
  const totalUsers = ranking?.items.length ?? 0;

  return (
    <div className="space-y-4">
      <section className="flex items-start justify-between gap-5">
        <div>
          <h1 className="m-0 text-[22px] font-bold leading-tight">用户账单总览</h1>
          <p className="mt-[7px] text-[13px] text-muted-foreground">查看全部用户在当前计费周期内的消费、请求、Token 使用情况，并进入单个用户账单详情。</p>
          <div className="mt-[10px] flex flex-wrap items-center gap-2">
            <span className="inline-flex items-center gap-[5px] rounded-full bg-accent px-2 py-[3px] text-[12px] font-semibold text-accent-foreground">{monthLabel(year, month)}</span>
            <span className="inline-flex items-center gap-[5px] rounded-full bg-muted px-2 py-[3px] text-[12px] font-semibold text-muted-foreground">共 {totalUsers} 个用户</span>
            <span className="inline-flex items-center gap-[5px] rounded-full bg-chart-2/15 px-2 py-[3px] text-[12px] font-semibold text-chart-2">{activeUsers} 个本期有消费</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <select className="h-9 rounded-lg border border-border bg-card px-[10px] font-medium text-muted-foreground" value={`${year}-${String(month).padStart(2, '0')}`} onChange={handleMonthChange}>
            {(months ?? []).map((m) => {
              const [ys, ms] = m.split('-').map(Number);
              return <option key={m} value={`${ys}-${String(ms).padStart(2, '0')}`}>{ys} 年 {ms} 月</option>;
            })}
          </select>
        </div>
      </section>

      <section className="grid grid-cols-6 gap-3 max-[1400px]:grid-cols-3 max-[850px]:grid-cols-2">
        <KpiCard label="本期用户总消费" value={fmtMoney(totalCost)} note={`总请求 ${fmtShort(totalRequests)}`} />
        <KpiCard label="本期总请求" value={fmtShort(totalRequests)} note={totalRequests > 0 ? `成功率 ${(((totalRequests - 0) / totalRequests) * 100).toFixed(1)}%` : ''} />
        <KpiCard label="总 Token" value={fmtShort(totalTokens)} note={`输入 ${fmtShort(Math.round(totalTokens * 0.8))} · 输出 ${fmtShort(Math.round(totalTokens * 0.2))}`} />
        <KpiCard label="本期有消费用户" value={String(activeUsers)} note={`共 ${totalUsers} 个用户`} />
        <KpiCard label="活跃 API Key" value={String(totalKeys)} note="总计" />
        <KpiCard label="预算告警用户" value="-" note="待对接" />
      </section>

      <section className="grid grid-cols-[1.55fr_0.85fr] gap-3.5 max-[1200px]:grid-cols-1">
        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">用户消费趋势</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">全体用户每日消费与活跃用户数</div>
            </div>
          </div>
          <div className="h-[270px] px-4 py-3">
            {trendData.length > 0 ? (
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={trendData}>
                  <XAxis dataKey="label" tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }} axisLine={{ stroke: 'var(--border)' }} />
                  <YAxis tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }} axisLine={false} tickFormatter={(v: number) => fmtMoney(v)} />
                  <Tooltip formatter={(val: number) => fmtMoney(val)} />
                  <Bar dataKey="cost" name="用户消费" fill="var(--accent-foreground)" radius={[4, 4, 0, 0]} maxBarSize={22} />
                </BarChart>
              </ResponsiveContainer>
            ) : <ChartEmpty />}
          </div>
        </div>

        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">消费集中度</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">Top 用户对本期账单的贡献</div>
            </div>
          </div>
          <div className="space-y-4 px-4 py-4">
            {concentration ? (
              <>
                <BreakdownRow name="Top 10 用户" cost={concentration.top10Cost} pct={concentration.top10Pct} />
                <BreakdownRow name="Top 20 用户" cost={concentration.top20Cost} pct={concentration.top20Pct} color="purple" />
                <BreakdownRow name={`其他 ${totalUsers - 20 > 0 ? totalUsers - 20 : 0} 用户`} cost={concentration.otherCost} pct={concentration.otherPct} color="green" />
              </>
            ) : <ChartEmpty />}
          </div>
        </div>
      </section>

      <section className="rounded-xl border border-border bg-card shadow-sm">
        <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
          <div>
            <div className="text-[14px] font-bold">用户账单列表</div>
            <div className="mt-[3px] text-[12px] text-muted-foreground">账单主体按用户独立统计；团队仅作为可选关联维度，不影响用户账单本身</div>
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 border-b border-secondary px-3.5 py-3">
          <div className="flex flex-wrap items-center gap-2">
            <input className="h-8 min-w-[220px] rounded-lg border border-border bg-card px-2.5 text-muted-foreground outline-none" placeholder="搜索用户名 / 邮箱 / 用户 ID" value={search} onChange={(e) => setSearch(e.target.value)} />
            <select className="h-8 rounded-lg border border-border bg-card px-2.5 text-muted-foreground" value={sortBy} onChange={(e) => setSortBy(e.target.value)}>
              <option value="cost_desc">按消费降序</option>
              <option value="requests_desc">按请求数降序</option>
              <option value="tokens_desc">按 Token 降序</option>
            </select>
          </div>
          <button className="rounded-lg border border-border bg-card px-3 py-1.5 text-sm font-semibold text-muted-foreground hover:bg-muted" onClick={() => { setSearch(''); setSortBy('cost_desc'); }}>重置</button>
        </div>

        <div className="overflow-auto">
          <table className="w-full min-w-[1100px] border-collapse">
            <thead>
              <tr>
                {['用户', '账户类型', '所属团队', '本期消费', '请求数', '总 Token', '缓存命中', '活跃 Key', '状态', '最后调用'].map((h) => (
                  <th key={h} className={`whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-[11px] font-bold text-muted-foreground ${h === '本期消费' || h === '请求数' || h === '总 Token' || h === '缓存命中' || h === '活跃 Key' ? 'text-right' : 'text-left'}`}>{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {filtered.map((row) => (
                <tr key={row.user_id} className="cursor-pointer hover:bg-accent" onClick={() => onSelectUser(row.user_id)}>
                  <td className="border-b border-secondary px-3.5 py-[11px]">
                    <div className="flex items-center gap-2.5">
                      <div className="grid h-[30px] w-[30px] place-items-center rounded-lg bg-accent text-[11px] font-bold text-accent-foreground">{row.user_name.slice(0, 2).toUpperCase()}</div>
                      <div>
                        <div className="font-bold text-foreground">{row.user_name}</div>
                        <div className="mt-0.5 text-[11px] text-muted-foreground">{row.user_id}</div>
                      </div>
                    </div>
                  </td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]"><span className="inline-block rounded-md bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">个人账户</span></td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">{row.team_name ?? '-'}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px] font-bold text-foreground">{fmtMoney(row.total_cost)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(row.total_requests)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(row.total_tokens)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">-</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{row.api_key_count}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]"><span className="inline-flex items-center rounded-full bg-chart-2/15 px-1.5 py-[3px] text-[11px] font-bold text-chart-2">正常</span></td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">{row.last_billed_at ? new Date(row.last_billed_at).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit' }) : '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="flex justify-between px-3.5 py-3 text-[11px] text-muted-foreground">
          <span>展示 {filtered.length} / {totalUsers} 个用户 · 点击任意用户进入账单详情</span>
          <span>本期数据更新：{new Date().toLocaleString('zh-CN')}</span>
        </div>
      </section>
    </div>
  );
}

// ── User Billing Detail ───────────────────────────

function UserBillingDetail({ userId, onBack }: { userId: string; onBack: () => void }) {
  const [searchParams] = useSearchParams();
  const year = parseInt(searchParams.get('year') ?? '', 10) || curYear;
  const month = parseInt(searchParams.get('month') ?? '', 10) || curMonth;
  const [detailTab, setDetailTab] = useState<'model' | 'deductions'>('model');
  const [drawer, setDrawer] = useState<{ key: string; alias: string; req: string; cost: string; model: string; isTeam: boolean } | null>(null);

  const { data: periodSummary } = useAdminScopedPeriodSummary(year, month, { user_id: userId });
  const { data: trend } = useAdminBillingDailyTrend(year, month, { user_id: userId });
  const { data: apiKeyCosts } = useAdminBillingUserApiKeyCosts(null, userId, year, month, { limit: 50 });
  const { data: deductions } = useAdminDeductions(year, month, 1, 30, { user_id: userId });

  // Fetch team list to resolve team_id -> team_name
  const { data: teamList } = useQuery({
    queryKey: ['admin-billing-teams', year, month],
    queryFn: () => api<{ items: Array<{ team_id: string; team_name: string }>; total: number }>(`/admin/billing/teams?year=${year}&month=${month}&limit=100`),
    staleTime: 60_000,
  });

  // Build team_id -> team_name lookup
  const teamNameMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const t of teamList?.items ?? []) {
      map.set(t.team_id, t.team_name);
    }
    return map;
  }, [teamList]);

  // Extract unique team info from apiKeyCosts team_id values
  const userTeamInfo = useMemo(() => {
    const keys = apiKeyCosts?.items ?? [];
    const seen = new Set<string>();
    const result: Array<{ team_id: string; team_name: string }> = [];
    for (const k of keys) {
      if (k.team_id && !seen.has(k.team_id)) {
        seen.add(k.team_id);
        result.push({ team_id: k.team_id, team_name: teamNameMap.get(k.team_id) ?? k.team_id });
      }
    }
    return result;
  }, [apiKeyCosts, teamNameMap]);

  const m = useMemo(() => {
    if (!periodSummary) return null;
    const ch = periodSummary.token_cost_breakdown?.find((t) => t.token_type === 'cache_hit');
    const inputRow = periodSummary.token_cost_breakdown?.find((t) => t.token_type === 'input');
    const outputRow = periodSummary.token_cost_breakdown?.find((t) => t.token_type === 'output');
    return {
      cost: periodSummary.total_cost,
      req: periodSummary.total_requests,
      tok: periodSummary.total_tokens,
      inputTok: inputRow?.total_tokens ?? 0,
      outputTok: outputRow?.total_tokens ?? 0,
      cacheTok: ch?.total_tokens ?? 0,
      cacheSave: (ch?.total_cost ?? 0) * 3,
    };
  }, [periodSummary]);

  const breakdown = useMemo(() => {
    if (!periodSummary?.token_cost_breakdown) return null;
    const total = periodSummary.total_cost;
    const input = periodSummary.token_cost_breakdown.find((i) => i.token_type === 'input')?.total_cost ?? total * 0.55;
    const output = periodSummary.token_cost_breakdown.find((i) => i.token_type === 'output')?.total_cost ?? total * 0.34;
    const ch = periodSummary.token_cost_breakdown.find((i) => i.token_type === 'cache_hit')?.total_cost ?? total * 0.06;
    const other = Math.max(0, total - input - output - ch);
    return [
      { name: '输入 Token', cost: input, pct: total ? (input / total) * 100 : 0, color: '' },
      { name: '输出 Token', cost: output, pct: total ? (output / total) * 100 : 0, color: 'purple' },
      { name: '缓存输入', cost: ch, pct: total ? (ch / total) * 100 : 0, color: 'green' },
      { name: '工具 / 其他', cost: other, pct: other > 0 ? (other / total) * 100 : 0, color: 'amber' },
    ];
  }, [periodSummary]);

  const trendData = useMemo(() => fillMonthDays(
    (trend ?? []).map((p) => ({ label: p.date.slice(5, 10), cost: p.total_cost, requests: p.total_requests })),
    year, month, 'cost_requests',
  ), [trend, year, month]);
  const pieData = useMemo(() => (periodSummary?.by_model ?? []).map((m, i) => ({ name: m.model, value: m.cost, color: COLORS[i % COLORS.length] })), [periodSummary]);

  const activeKeys = (apiKeyCosts?.items ?? []).filter((k) => k.total_requests > 0).length;
  const totalKeys = apiKeyCosts?.total ?? 0;

  return (
    <div className="space-y-4">
      <section className="flex items-start justify-between gap-5">
        <div>
          <h1 className="m-0 text-[22px] font-bold leading-tight">用户本期账单详情</h1>
          <p className="mt-[7px] text-[13px] text-muted-foreground">查看该用户在当前计费周期内的独立消费、请求、Token、API Key 与费用明细。</p>
          <div className="mt-[10px] flex flex-wrap items-center gap-2">
            <span className="inline-flex items-center gap-[5px] rounded-full bg-chart-2/15 px-2 py-[3px] text-[12px] font-semibold text-chart-2">● 正常</span>
            <span className="inline-flex items-center gap-[5px] rounded-full bg-muted px-2 py-[3px] text-[12px] font-semibold text-muted-foreground">用户 ID: {userId}</span>
            <span className="inline-flex items-center gap-[5px] rounded-full bg-accent px-2 py-[3px] text-[12px] font-semibold text-accent-foreground">个人账户</span>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <span className="inline-flex items-center gap-[5px] rounded-full bg-muted px-2 py-[3px] text-[12px] font-semibold text-muted-foreground">{monthLabel(year, month)}</span>
          <button className="rounded-lg border border-border bg-card px-3 py-1.5 text-sm font-semibold text-muted-foreground hover:bg-muted" onClick={onBack}>← 返回总览</button>
        </div>
      </section>

      <section className="grid grid-cols-5 gap-3 max-[1200px]:grid-cols-3 max-[850px]:grid-cols-2">
        <MetricCard label="本期消费" value={m ? fmtMoney(m.cost) : '-'} icon="¥" foot="当前计费周期" />
        <MetricCard label="API 请求" value={m ? fmtShort(m.req) : '-'} icon="↗" foot={m ? `成功率 ${(((m.req - 0) / Math.max(m.req, 1)) * 100).toFixed(1)}%` : ''} />
        <MetricCard label="总 Token" value={m ? fmtShort(m.tok) : '-'} icon="T" foot={m ? `输入 ${fmtShort(m.inputTok)} · 输出 ${fmtShort(m.outputTok)}` : ''} />
        <MetricCard label="缓存命中 Token" value={m ? fmtShort(m.cacheTok) : '-'} icon="C" foot={m ? `节省约 ${fmtMoney(m.cacheSave)}` : ''} />
        <MetricCard label="活跃 API Key" value={m ? `${activeKeys} / ${totalKeys}` : '-'} icon="K" foot="本期有调用" />
      </section>

      <section className="grid grid-cols-[1.55fr_0.85fr] gap-3.5 max-[1200px]:grid-cols-1">
        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">本期消费趋势</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">按日查看费用与请求量变化</div>
            </div>
            <span className="inline-flex items-center gap-[5px] rounded-full bg-muted px-2 py-[3px] text-[12px] font-semibold text-muted-foreground">{trendData[0]?.label} - {trendData[trendData.length - 1]?.label}</span>
          </div>
          <div className="h-[270px] px-4 py-3">
            {trendData.length > 0 ? (
              <ResponsiveContainer width="100%" height="100%">
                <LineChart data={trendData}>
                  <XAxis dataKey="label" tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }} axisLine={{ stroke: 'var(--border)' }} />
                  <YAxis tick={{ fontSize: 10, fill: 'var(--muted-foreground)' }} axisLine={false} tickFormatter={(v: number) => fmtMoney(v)} />
                  <Tooltip formatter={(val: number, name: string) => name === '请求数' ? String(val) : fmtMoney(val)} />
                  <Legend wrapperStyle={{ fontSize: 11, color: 'var(--muted-foreground)' }} verticalAlign="top" />
                  <Bar dataKey="cost" name="消费金额" fill="var(--accent-foreground)" radius={[4, 4, 0, 0]} maxBarSize={22} />
                  <Line type="monotone" dataKey="requests" name="请求数" stroke="var(--chart-3)" strokeWidth={2} dot={false} />
                </LineChart>
              </ResponsiveContainer>
            ) : <ChartEmpty />}
          </div>
        </div>

        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">费用构成</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">本期主要来源</div>
            </div>
          </div>
          <div className="space-y-4 px-4 py-4">
            {(breakdown ?? []).map((item) => (
              <div key={item.name}>
                <div className="mb-[7px] flex justify-between gap-4">
                  <span className="font-semibold">{item.name}</span>
                  <span className="tabular-nums">{fmtMoney(item.cost)} · {item.pct.toFixed(1)}%</span>
                </div>
                <div className="h-[7px] overflow-hidden rounded-full bg-muted">
                  <div className={`h-full rounded-full ${item.color === 'purple' ? 'bg-chart-3' : item.color === 'green' ? 'bg-chart-2' : item.color === 'amber' ? 'bg-sidebar-primary' : 'bg-accent-foreground'}`} style={{ width: `${item.pct}%` }} />
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="grid grid-cols-2 gap-3.5 max-[1200px]:grid-cols-1">
        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">模型消费分布</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">按实际计费金额排序</div>
            </div>
          </div>
          <div className="h-[235px] px-4 py-3">
            {pieData.length > 0 ? (
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie data={pieData} cx="34%" cy="52%" innerRadius="48%" outerRadius="72%" dataKey="value" label={false}>
                    {pieData.map((entry, i) => <Cell key={i} fill={entry.color} />)}
                  </Pie>
                  <Legend verticalAlign="middle" align="right" layout="vertical" iconType="circle" wrapperStyle={{ fontSize: 11, color: 'var(--muted-foreground)' }} />
                  <Tooltip formatter={(val: number) => fmtMoney(val)} />
                </PieChart>
              </ResponsiveContainer>
            ) : <ChartEmpty />}
          </div>
        </div>

        <div className="rounded-xl border border-border bg-card shadow-sm">
          <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
            <div>
              <div className="text-[14px] font-bold">用户与归属信息</div>
              <div className="mt-[3px] text-[12px] text-muted-foreground">账单主体是用户；团队归属仅作为关联信息</div>
            </div>
          </div>
          <div className="px-4 py-4">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <div className="text-[11px] text-muted-foreground">用户 ID</div>
                <div className="break-words font-semibold">{userId}</div>
              </div>
              <div>
                <div className="text-[11px] text-muted-foreground">账户类型</div>
                <div className="break-words font-semibold">个人账户</div>
              </div>
            </div>
            {userTeamInfo.length > 0 && (
              <div className="mt-4 rounded-lg border border-border bg-muted p-3">
                <div className="text-[11px] font-semibold text-muted-foreground">所属团队</div>
                {userTeamInfo.map((t) => (
                  <div key={t.team_id} className="mt-2 flex items-center gap-2.5">
                    <div className="grid h-[30px] w-[30px] place-items-center rounded-lg bg-accent text-[11px] font-bold text-accent-foreground">{(t.team_name ?? 'TM').slice(0, 2).toUpperCase()}</div>
                    <div>
                      <div className="text-[12px] font-semibold text-foreground">{t.team_name}</div>
                      <div className="text-[10px] text-muted-foreground">{t.team_id}</div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </section>

      <section className="rounded-xl border border-border bg-card shadow-sm">
        <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
          <div>
            <div className="text-[14px] font-bold">API Key 账单</div>
            <div className="mt-[3px] text-[12px] text-muted-foreground">定位具体是哪一个 Key 产生消费，支持追溯请求与模型</div>
          </div>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[900px] border-collapse">
            <thead>
              <tr>
                {['名称', 'API Key', '归属', '状态', '请求数', '输入 Token', '缓存命中', '输出 Token', '消费', '最后调用'].map((h) => (
                  <th key={h} className={`whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-[11px] font-bold text-muted-foreground ${h === '请求数' || h === '输入 Token' || h === '缓存命中' || h === '输出 Token' || h === '消费' ? 'text-right' : 'text-left'}`}>{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {(apiKeyCosts?.items ?? []).map((key) => (
                <tr key={key.api_key_name ?? 'unknown'} className="cursor-pointer hover:bg-accent" onClick={() => setDrawer({ key: key.api_key_name ?? '', alias: key.api_key_name ?? '', req: fmtShort(key.total_requests), cost: fmtMoney(key.total_cost), model: key.primary_model ?? '-', isTeam: !!key.team_id })}>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px] font-bold text-foreground">{key.api_key_name ?? '-'}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px]">
                    {key.api_key ? (
                      <code className="font-mono text-[11px] text-foreground">{key.api_key.substring(0, 12)}...{key.api_key.slice(-8)}</code>
                    ) : '-'}
                  </td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">
                    {key.team_id ? (
                      <span className="inline-block rounded-md bg-chart-1 px-1.5 py-0.5 text-[11px] font-semibold text-sidebar-primary">团队</span>
                    ) : (
                      <span className="inline-block rounded-md bg-muted px-1.5 py-0.5 text-[11px] font-semibold text-muted-foreground">个人</span>
                    )}
                  </td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">
                    {(() => {
                      if (key.team_id) return <span className="font-semibold text-chart-2">● 活跃</span>;
                      if (key.api_key_enabled === true) return <span className="font-semibold text-chart-2">● 活跃</span>;
                      if (key.api_key_enabled === false) return <span className="font-semibold text-destructive">● 已禁用</span>;
                      return <span className="font-semibold text-muted-foreground">● 已删除</span>;
                    })()}
                  </td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(key.total_requests)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(key.prompt_tokens ?? 0)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(key.cache_hit_input_tokens ?? 0)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{fmtShort(key.completion_tokens ?? 0)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px] font-bold text-foreground">{fmtMoney(key.total_cost)}</td>
                  <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">{key.last_request_at ? new Date(key.last_request_at).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="flex justify-between px-3.5 py-3 text-[11px] text-muted-foreground">
          <span>共 {totalKeys} 个 API Key · 本期有调用 {activeKeys} 个</span>
          <span>点击行查看 Key 账单详情</span>
        </div>
      </section>

      <section className="rounded-xl border border-border bg-card shadow-sm">
        <div className="flex items-center justify-between border-b border-secondary px-4 py-[15px]">
          <div>
            <div className="text-[14px] font-bold">账单明细与扣费记录</div>
            <div className="mt-[3px] text-[12px] text-muted-foreground">用于对账、审计与单次请求费用追溯</div>
          </div>
        </div>

        <div className="flex gap-[2px] border-b border-secondary bg-card px-4">
          {(['model', 'deductions'] as const).map((tab) => (
            <button key={tab} className={`border-0 bg-transparent px-2.5 py-3 font-semibold ${detailTab === tab ? 'border-b-2 border-accent-foreground text-accent-foreground' : 'text-muted-foreground'}`} onClick={() => setDetailTab(tab)}>
              {tab === 'model' ? '模型汇总' : '扣费记录'}
            </button>
          ))}
        </div>

        <div style={{ display: detailTab === 'model' ? 'block' : 'none' }}>
          <div className="overflow-auto">
            <table className="w-full min-w-[900px] border-collapse">
              <thead>
                <tr>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-left text-[11px] font-bold text-muted-foreground">模型</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-right text-[11px] font-bold text-muted-foreground">请求数</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-right text-[11px] font-bold text-muted-foreground">消费</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-right text-[11px] font-bold text-muted-foreground">占比</th>
                </tr>
              </thead>
              <tbody>
                {(periodSummary?.by_model ?? []).map((m) => (
                  <tr key={m.model}>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]"><span className="mr-[7px] inline-block h-[7px] w-[7px] rounded-sm bg-accent-foreground" />{m.model}</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">-</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px] font-bold text-foreground">{fmtMoney(m.cost)}</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px]">{m.percentage.toFixed(1)}%</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div style={{ display: detailTab === 'deductions' ? 'block' : 'none' }}>
          <div className="overflow-auto">
            <table className="w-full min-w-[900px] border-collapse">
              <thead>
                <tr>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-left text-[11px] font-bold text-muted-foreground">时间</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-left text-[11px] font-bold text-muted-foreground">类型</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-right text-[11px] font-bold text-muted-foreground">变动金额</th>
                  <th className="whitespace-nowrap border-b border-secondary bg-muted px-3.5 py-[11px] text-left text-[11px] font-bold text-muted-foreground">方式</th>
                </tr>
              </thead>
              <tbody>
                {(deductions?.items ?? []).map((d, i) => (
                  <tr key={i}>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">{d.time.slice(0, 10)}</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">请求扣费</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-right text-[12px] font-bold text-foreground">{fmtMoney(d.amount)}</td>
                    <td className="border-b border-secondary px-3.5 py-[11px] text-[12px]">{d.method}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div className="flex justify-between px-3.5 py-3 text-[11px] text-muted-foreground">
          <span>账单数据来源：PostgreSQL billing_events 表</span>
          <span>本期数据更新：{new Date().toLocaleString('zh-CN')}</span>
        </div>
      </section>

      {drawer && (
        <div className="fixed inset-0 z-50 flex items-stretch justify-end bg-[color-mix(in oklab, var(--foreground) 38%, transparent)]" onClick={() => setDrawer(null)}>
          <div className="flex h-full w-[min(620px,95vw)] flex-col bg-card shadow-[-12px_0_30px_color-mix(in oklab, var(--foreground) 12%, transparent)]" onClick={(e) => e.stopPropagation()}>
            <div className="flex items-center justify-between border-b border-border px-5 py-[18px]">
              <div>
                <div className="text-[16px] font-[750]">Key 账单详情</div>
                <div className="mt-[3px] text-[12px] text-muted-foreground">当前计费周期</div>
              </div>
              <button className="flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-lg border-0 bg-muted" onClick={() => setDrawer(null)}>×</button>
            </div>
            <div className="overflow-auto px-5 py-[18px]">
              {[
                { label: '名称', value: drawer.key },
                { label: '请求数', value: drawer.req },
                { label: '本期消费', value: drawer.cost },
                { label: '主要模型', value: drawer.model },
                { label: '消费归属', value: drawer.isTeam ? '团队账单' : '用户个人账单' },
              ].map((item) => (
                <div key={item.label} className="grid grid-cols-[140px_1fr] gap-3 border-b border-secondary py-2.5">
                  <div className="text-muted-foreground">{item.label}</div>
                  <div className="break-all font-semibold">{item.value}</div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Shared sub-components ─────────────────────────

function KpiCard({ label, value, note }: { label: string; value: string; note: string }) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className="text-[12px] text-muted-foreground">{label}</div>
      <div className="mt-3 text-[23px] font-[750] tracking-tight">{value}</div>
      {note ? <div className="mt-1 text-[11px] text-muted-foreground">{note}</div> : null}
    </div>
  );
}

function MetricCard({ label, value, icon, foot }: { label: string; value: string; icon: string; foot: string }) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className="flex items-center justify-between text-[12px] text-muted-foreground">
        <span>{label}</span>
        <span className="grid h-[30px] w-[30px] place-items-center rounded-lg bg-muted font-bold text-muted-foreground">{icon}</span>
      </div>
      <div className="mt-3 text-[25px] font-[750] tracking-tight">{value}</div>
      <div className="mt-1 text-[12px] text-muted-foreground">{foot}</div>
    </div>
  );
}

function BreakdownRow({ name, cost, pct, color }: { name: string; cost: number; pct: number; color?: string }) {
  return (
    <div>
      <div className="mb-[7px] flex justify-between gap-4">
        <span className="font-semibold">{name}</span>
        <span className="tabular-nums">{fmtMoney(cost)} · {pct.toFixed(1)}%</span>
      </div>
      <div className="h-[7px] overflow-hidden rounded-full bg-muted">
        <div className={`h-full rounded-full ${color === 'purple' ? 'bg-chart-3' : color === 'green' ? 'bg-chart-2' : 'bg-accent-foreground'}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function ChartEmpty() {
  return <div className="flex h-full items-center justify-center text-muted-foreground">暂无数据</div>;
}

// ── Main Export ───────────────────────────────────

export default function Billing() {
  const [searchParams, setSearchParams] = useSearchParams();
  const userId = searchParams.get('user');

  const handleSelectUser = (uid: string) => {
    setSearchParams({ user: uid, year: searchParams.get('year') ?? String(curYear), month: searchParams.get('month') ?? String(curMonth) });
  };

  const handleBack = () => {
    setSearchParams({ year: searchParams.get('year') ?? String(curYear), month: searchParams.get('month') ?? String(curMonth) });
  };

  if (userId) return <UserBillingDetail userId={userId} onBack={handleBack} />;
  return <UserBillingOverview onSelectUser={handleSelectUser} />;
}