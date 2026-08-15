import type { BillingCopy } from './types';
import type { AdminBillingTeamRow } from '@fluxeme/shared/src/types';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';

type Props = {
  copy: BillingCopy;
  items: AdminBillingTeamRow[];
  selectedTeamId: string | null;
  onSelectTeam: (teamId: string) => void;
  onOpenTeam: (teamId: string) => void;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};


export function TeamsView({ copy, items, selectedTeamId, onSelectTeam, onOpenTeam, fmtCurrency, compactNumber }: Props) {
  const kpis: KpiItem[] = [
    { label: '有消费团队', value: String(items.length) },
    { label: '团队总消费', value: fmtCurrency(items.reduce((s, t) => s + t.total_cost, 0)) },
    { label: '团队请求', value: compactNumber(items.reduce((s, t) => s + t.total_requests, 0)) },
    { label: '团队 Token', value: compactNumber(items.reduce((s, t) => s + t.total_tokens, 0)) },
  ];

  return (
    <div className="space-y-[14px]">
      <BillingKpiRow items={kpis} />

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div>
            <div className="text-[13px] font-[700] text-[#182033]">团队账单</div>
            <div className="mt-[3px] text-[9px] text-[#778296]">团队作为独立账单主体查看</div>
          </div>
          <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{items.length} 个有消费团队</span>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[800px] border-collapse">
            <thead>
              <tr>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">团队</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">消费</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">成员</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">API Key</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">操作</th>
              </tr>
            </thead>
            <tbody>
              {items.length > 0 ? items.map((t) => (
                <tr key={t.team_id} className={`cursor-pointer ${selectedTeamId === t.team_id ? 'bg-[#f2f4ff]' : 'hover:bg-[#fbfcff]'}`} onClick={() => { onSelectTeam(t.team_id); onOpenTeam(t.team_id); }}>
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">
                    <div className="font-[680] text-[#182033]">{t.team_name}</div>
                    <div className="mt-[2px] text-[8px] text-[#99a3b3]">{t.team_id}</div>
                  </td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(t.total_cost)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(t.total_requests)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(t.total_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{t.active_users}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">—</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px]">
                    <button type="button" onClick={(e) => { e.stopPropagation(); onOpenTeam(t.team_id); }} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">查看团队账单 →</button>
                  </td>
                </tr>
              )) : <tr><td colSpan={7} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">{copy.noTeams}</td></tr>}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
