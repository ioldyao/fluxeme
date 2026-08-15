import type { BillingCopy, BillingView } from './types';
import type { PeriodSummary } from '@fluxeme/shared/src/api/billing';
import type { AdminBillingTeamRow, AdminBillingTeamUserRow } from '@fluxeme/shared/src/types';
import { EntityHead } from './EntityHead';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';
import { BreakdownBar } from './BreakdownBar';

type Props = {
  copy: BillingCopy;
  onViewChange: (view: BillingView) => void;
  team: AdminBillingTeamRow;
  scopedPeriod?: PeriodSummary;
  members: AdminBillingTeamUserRow[];
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

export function TeamDetailView({ onViewChange, team, scopedPeriod, members, fmtCurrency, compactNumber }: Props) {
  const inputRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'input');
  const cacheRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'cache_hit');
  const outputRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'output');

  const kpis: KpiItem[] = [
    { label: '本期消费', value: scopedPeriod ? fmtCurrency(scopedPeriod.total_cost) : '—' },
    { label: '请求数', value: scopedPeriod ? compactNumber(scopedPeriod.total_requests) : '—' },
    { label: 'Token 使用量', value: scopedPeriod ? compactNumber(scopedPeriod.total_tokens) : '—' },
    { label: 'API Key 数量', value: String(team.active_users) },
  ];

  const breakdownItems = [
    { label: '输入费用', value: `${fmtCurrency(inputRow?.total_cost ?? 0)} · ${(inputRow?.percentage ?? 0).toFixed(0)}%`, pct: inputRow?.percentage ?? 0 },
    { label: '缓存命中费用', value: `${fmtCurrency(cacheRow?.total_cost ?? 0)} · ${(cacheRow?.percentage ?? 0).toFixed(0)}%`, pct: cacheRow?.percentage ?? 0 },
    { label: '输出费用', value: `${fmtCurrency(outputRow?.total_cost ?? 0)} · ${(outputRow?.percentage ?? 0).toFixed(0)}%`, pct: outputRow?.percentage ?? 0 },
  ];

  return (
    <div className="space-y-[14px]">
      <div className="flex items-center gap-[6px] text-[10px] text-[#8792a3]">
        <button type="button" onClick={() => onViewChange('teams')} className="border-0 bg-transparent p-0 text-[10px] text-[#5268f6]">团队账单</button>
        <span>/</span><b className="text-[#283448]">{team.team_name}</b>
      </div>

      <EntityHead avatar={team.team_name.charAt(0)} name={team.team_name} meta={`${team.team_id} · 团队账单`} />

      <BillingKpiRow items={kpis} />

      <div className="grid gap-[14px] xl:grid-cols-[1.3fr_.7fr]">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">团队成员使用</div><div className="mt-[3px] text-[9px] text-[#778296]">谁在团队账单下产生费用</div></div>
          </div>
          <div className="overflow-auto">
            <table className="w-full min-w-[700px] border-collapse">
              <thead>
                <tr>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">用户</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">团队内消费</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                  <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">最近活跃</th>
                </tr>
              </thead>
              <tbody>
                {members.length > 0 ? members.map((m) => (
                  <tr key={m.user_id} className="hover:bg-[#fbfcff]">
                    <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]"><div className="font-[680] text-[#182033]">{m.user_name}</div></td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(m.total_cost)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(m.total_requests)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(m.total_tokens)}</td>
                    <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] text-[#7b8496]">{m.last_billed_at || '—'}</td>
                  </tr>
                )) : <tr><td colSpan={5} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">暂无成员使用记录</td></tr>}
              </tbody>
            </table>
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">团队费用构成</div></div>
          </div>
          <div className="px-[14px] py-[14px]"><BreakdownBar items={breakdownItems} /></div>
        </div>
      </div>
    </div>
  );
}
