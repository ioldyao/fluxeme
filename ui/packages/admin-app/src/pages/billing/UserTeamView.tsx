import type { BillingView } from './types';
import type { AdminBillingApiKeyActivityRow } from '@fluxeme/shared/src/types';
import { EntityHead } from './EntityHead';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';

type Props = {
  onViewChange: (view: BillingView) => void;
  userName: string;
  teamName: string;
  usage: { cost: number; requests: number; tokens: number; keysCount: number };
  items: AdminBillingApiKeyActivityRow[];
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

export function UserTeamView({ onViewChange, userName, teamName, usage, items, fmtCurrency, compactNumber }: Props) {
  const kpis: KpiItem[] = [
    { label: '团队内消费', value: fmtCurrency(usage.cost) },
    { label: '团队内请求', value: compactNumber(usage.requests) },
    { label: '团队内 Token', value: compactNumber(usage.tokens) },
    { label: '团队 API Key', value: String(usage.keysCount) },
  ];

  return (
    <div className="space-y-[14px]">
      <div className="flex items-center gap-[6px] text-[10px] text-[#8792a3]">
        <button type="button" onClick={() => onViewChange('user-detail')} className="border-0 bg-transparent p-0 text-[10px] text-[#5268f6]">个人用户账单</button>
        <span>/</span><b className="text-[#283448]">{userName}</b><span>/</span><b className="text-[#283448]">{teamName}</b>
      </div>
      <EntityHead avatar={userName.charAt(0).toUpperCase()} name={`${userName} 在 ${teamName} 下的使用`} meta="仅查看此用户在该团队 Billing Account 下产生的使用" />
      <BillingKpiRow items={kpis} />
      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div><div className="text-[13px] font-[700] text-[#182033]">该用户在此团队下的使用明细</div><div className="mt-[3px] text-[9px] text-[#778296]">关联使用视角，不改变个人账单独立性</div></div>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[700px] border-collapse">
            <thead>
              <tr>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">API Key</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">最近活跃</th>
              </tr>
            </thead>
            <tbody>
              {items.length > 0 ? items.map((row) => (
                <tr key={row.api_key_name} className="hover:bg-[#fbfcff]">
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] font-[680] text-[#182033]">{row.api_key_name || 'Unnamed Key'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_requests)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] text-[#7b8496]">{row.last_request_at || '—'}</td>
                </tr>
              )) : <tr><td colSpan={4} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">暂无使用记录</td></tr>}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
