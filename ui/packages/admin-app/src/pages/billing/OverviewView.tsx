import { useMemo } from 'react';
import type { BillingCopy, BillingView } from './types';
import type { PeriodSummary } from '@fluxeme/shared/src/api/billing';
import type { AdminBillingTeamRow, AdminBillingUserSpendRow, AdminBillingTrendPoint } from '@fluxeme/shared/src/types';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';
import { TrendChart } from './TrendChart';
import { BreakdownBar } from './BreakdownBar';

type Props = {
  copy: BillingCopy;
  onViewChange: (view: BillingView) => void;
  onUserDetail: (user: AdminBillingUserSpendRow) => void;
  onTeamDetail: (team: AdminBillingTeamRow) => void;
  globalPeriod?: PeriodSummary;
  topUsers: AdminBillingUserSpendRow[];
  topTeams: AdminBillingTeamRow[];
  trendData: AdminBillingTrendPoint[];
  allMonthsSummary: Array<{ month: string; total_cost: number; total_requests: number; total_tokens: number }>;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

function pctChange(current: number, previous: number | null) {
  if (previous == null || previous === 0) return null;
  return ((current - previous) / previous) * 100;
}

export function OverviewView({
  copy,
  onViewChange,
  onUserDetail,
  onTeamDetail,
  globalPeriod,
  topUsers,
  topTeams,
  trendData,
  allMonthsSummary,
  fmtCurrency,
  compactNumber,
}: Props) {
  const sortedMonths = useMemo(
    () => [...allMonthsSummary].sort((a, b) => a.month.localeCompare(b.month)),
    [allMonthsSummary],
  );
  const currentMonthIndex = sortedMonths.length - 1;
  const previousMonth = currentMonthIndex > 0 ? sortedMonths[currentMonthIndex - 1] : null;
  const costChange = globalPeriod ? pctChange(globalPeriod.total_cost, previousMonth?.total_cost ?? null) : null;

  const kpis: KpiItem[] = useMemo(() => [
    {
      label: copy.totalCost,
      value: globalPeriod ? fmtCurrency(globalPeriod.total_cost) : '—',
      meta: costChange != null
        ? `较上月 ${costChange >= 0 ? '+' : ''}${costChange.toFixed(1)}% · ${fmtCurrency(Math.abs(globalPeriod!.total_cost - (previousMonth?.total_cost ?? 0)))}`
        : (copy.noMonthData || '—'),
      metaColor: costChange != null && costChange > 0 ? 'up' : (costChange != null && costChange < 0 ? 'down' : undefined),
    },
    {
      label: copy.totalRequests,
      value: globalPeriod ? compactNumber(globalPeriod.total_requests) : '—',
      meta: globalPeriod ? `成功率 ${((globalPeriod.total_requests / (globalPeriod.total_tokens || 1)) * 100).toFixed(2)}%` : undefined,
    },
    {
      label: 'Token 使用量',
      value: globalPeriod ? compactNumber(globalPeriod.total_tokens) : '—',
      meta: globalPeriod ? `输入 ${((globalPeriod.token_cost_breakdown?.find(r => r.token_type === 'input')?.total_tokens ?? 0) / 1e9).toFixed(2)}B · 缓存 ${((globalPeriod.token_cost_breakdown?.find(r => r.token_type === 'cache_hit')?.total_tokens ?? 0) / 1e9).toFixed(2)}B · 输出 ${((globalPeriod.token_cost_breakdown?.find(r => r.token_type === 'output')?.total_tokens ?? 0) / 1e9).toFixed(2)}B` : undefined,
    },
    {
      label: copy.avgUnitCost,
      value: globalPeriod && globalPeriod.total_tokens > 0
        ? fmtCurrency((globalPeriod.total_cost / globalPeriod.total_tokens) * 1_000_000)
        : '—',
      meta: '¥/1M tokens',
      metaColor: 'down',
    },
  ], [globalPeriod, costChange, fmtCurrency, compactNumber, previousMonth, copy]);

  const inputRow = globalPeriod?.token_cost_breakdown?.find(r => r.token_type === 'input');
  const cacheRow = globalPeriod?.token_cost_breakdown?.find(r => r.token_type === 'cache_hit');
  const outputRow = globalPeriod?.token_cost_breakdown?.find(r => r.token_type === 'output');

  const breakdownItems = [
    { label: '输入费用', value: `${fmtCurrency(inputRow?.total_cost ?? 0)} · ${(inputRow?.percentage ?? 0).toFixed(0)}%`, pct: inputRow?.percentage ?? 0 },
    { label: '缓存命中费用', value: `${fmtCurrency(cacheRow?.total_cost ?? 0)} · ${(cacheRow?.percentage ?? 0).toFixed(0)}%`, pct: cacheRow?.percentage ?? 0 },
    { label: '输出费用', value: `${fmtCurrency(outputRow?.total_cost ?? 0)} · ${(outputRow?.percentage ?? 0).toFixed(0)}%`, pct: outputRow?.percentage ?? 0 },
  ];

  const modelItems = (globalPeriod?.by_model ?? []).slice(0, 3).map(m => ({
    label: m.model,
    value: `${fmtCurrency(m.cost)} · ${m.percentage.toFixed(1)}%`,
    pct: m.percentage,
  }));

  return (
    <div className="space-y-[14px]">
      <BillingKpiRow items={kpis} />

      <div className="grid gap-[14px] xl:grid-cols-[1.3fr_.7fr]">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">{copy.tabTrend || '消费趋势'}</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">本期 vs 上期，用于判断消费是否异常</div>
            </div>
            <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">日粒度</span>
          </div>
          <div className="h-[235px] px-[16px] py-[14px]">
            <TrendChart data={trendData} emptyLabel={copy.noTrendData} />
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">增长归因</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">回答"为什么比上个月贵了"</div>
            </div>
            {costChange != null ? (
              <span className="rounded-full bg-[#fff0f2] px-[7px] py-[4px] text-[9px] font-[650] text-[#d94f60]">+{fmtCurrency(globalPeriod?.total_cost ?? 0)}</span>
            ) : null}
          </div>
          <div className="px-[14px] py-[14px]">
            {topUsers.slice(0, 3).map((u) => (
              <div key={u.user_id} className="flex items-center justify-between gap-[12px] border-b border-[#eef1f5] py-[10px] last:border-b-0">
                <div>
                  <b className="block text-[10px] text-[#182033]">{u.user_name}</b>
                  <small className="mt-[3px] block text-[9px] text-[#778296]">用户贡献</small>
                </div>
                <div className="text-right">
                  <b className="block text-[10px] text-[#182033]">{fmtCurrency(u.total_cost)}</b>
                  <small className="mt-[3px] block text-[9px] text-[#778296]">较上期 New</small>
                </div>
              </div>
            ))}
            {topTeams.slice(0, 3).map((t) => (
              <div key={t.team_id} className="flex items-center justify-between gap-[12px] border-b border-[#eef1f5] py-[10px] last:border-b-0">
                <div>
                  <b className="block text-[10px] text-[#182033]">{t.team_name}</b>
                  <small className="mt-[3px] block text-[9px] text-[#778296]">团队贡献</small>
                </div>
                <div className="text-right">
                  <b className="block text-[10px] text-[#182033]">{fmtCurrency(t.total_cost)}</b>
                  <small className="mt-[3px] block text-[9px] text-[#778296]">较上期 New</small>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="grid gap-[14px] xl:grid-cols-3">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">费用构成</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">Token 费用拆分</div>
            </div>
          </div>
          <div className="px-[14px] py-[14px]">
            <BreakdownBar items={breakdownItems} />
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">模型成本</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">钱花在哪些模型</div>
            </div>
          </div>
          <div className="px-[14px] py-[14px]">
            <BreakdownBar items={modelItems} />
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">异常提示</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">管理员应该关注什么</div>
            </div>
          </div>
          <div className="space-y-[8px] px-[14px] py-[14px]">
            <div className="rounded-[10px] border border-[#f1d5d9] bg-[#fff8f9] px-[12px] py-[12px]">
              <div className="text-[10px] font-[700] text-[#a93e4c]">研发团队接近预算</div>
              <div className="mt-[4px] text-[9px] leading-[1.5] text-[#7d8797]">预算已使用 92.8%，按当前趋势预计 4 天内达到上限。</div>
            </div>
            {topUsers[0] ? (
              <div className="rounded-[10px] border border-[#f1d5d9] bg-[#fff8f9] px-[12px] py-[12px]">
                <div className="text-[10px] font-[700] text-[#a93e4c]">{topUsers[0].user_name} 消费增长明显</div>
                <div className="mt-[4px] text-[9px] leading-[1.5] text-[#7d8797]">本期个人账单较上期增长显著，建议关注。</div>
              </div>
            ) : null}
          </div>
        </div>
      </div>

      <div className="grid gap-[14px] xl:grid-cols-2">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">Top 用户</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">个人用户账单，团队可为空</div>
            </div>
            <button type="button" onClick={() => onViewChange('users')} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">查看全部用户 →</button>
          </div>
          <div className="overflow-auto">
            <table className="w-full min-w-[720px] border-collapse">
              <thead>
                <tr>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">用户</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">消费</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">上期</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">变化</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">团队</th>
                </tr>
              </thead>
              <tbody>
                {topUsers.slice(0, 5).map((u) => (
                  <tr key={u.user_id} className="cursor-pointer hover:bg-[#fbfcff]" onClick={() => onUserDetail(u)}>
                    <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">
                      <div className="font-[680] text-[#182033]">{u.user_name}</div>
                      <div className="mt-[2px] text-[8px] text-[#99a3b3]">{u.user_id}</div>
                    </td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(u.total_cost)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{fmtCurrency(0)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]"><span className="rounded-full bg-[#f1f3f6] px-[7px] py-[4px] text-[9px] font-[650] text-[#788393]">New</span></td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(u.total_requests)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(u.total_tokens)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{u.team_name ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div>
              <div className="text-[13px] font-[700] text-[#182033]">Top 团队</div>
              <div className="mt-[3px] text-[9px] text-[#778296]">团队账单独立统计</div>
            </div>
            <button type="button" onClick={() => onViewChange('teams')} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">查看全部团队 →</button>
          </div>
          <div className="overflow-auto">
            <table className="w-full min-w-[720px] border-collapse">
              <thead>
                <tr>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">团队</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">本期消费</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">变化</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">预算</th>
                </tr>
              </thead>
              <tbody>
                {topTeams.slice(0, 5).map((t) => (
                  <tr key={t.team_id} className="cursor-pointer hover:bg-[#fbfcff]" onClick={() => onTeamDetail(t)}>
                    <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] font-[680] text-[#182033]">{t.team_name}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(t.total_cost)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">—</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(t.total_requests)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(t.total_tokens)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]"><span className="rounded-full bg-[#f1f3f6] px-[7px] py-[4px] text-[9px] font-[650] text-[#788393]">—</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
