import type { BillingCopy } from './types';
import type { AdminBillingUserApiKeyCostRow } from '@fluxeme/shared/src/types';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';

type Props = {
  copy: BillingCopy;
  items: AdminBillingUserApiKeyCostRow[];
  selectedApiKeyName: string | null;
  onSelectApiKey: (apiKeyName: string) => void;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

export function KeysView({ copy, items, selectedApiKeyName, onSelectApiKey, fmtCurrency, compactNumber }: Props) {
  const kpis: KpiItem[] = [
    { label: '有调用 Key', value: String(items.length) },
    { label: 'Key 总消费', value: fmtCurrency(items.reduce((s, k) => s + k.total_cost, 0)) },
    { label: 'Key 请求', value: compactNumber(items.reduce((s, k) => s + k.total_requests, 0)) },
    { label: 'Key Token', value: compactNumber(items.reduce((s, k) => s + k.total_tokens, 0)) },
  ];

  return (
    <div className="space-y-[14px]">
      <BillingKpiRow items={kpis} />

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div>
            <div className="text-[13px] font-[700] text-[#182033]">API Key 账单</div>
            <div className="mt-[3px] text-[9px] text-[#778296]">个人 Key 与团队 Key 并列查看</div>
          </div>
          <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{items.length} 个有调用 Key</span>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[900px] border-collapse">
            <thead>
              <tr>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">API Key</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">消费</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">Token</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">主要模型</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">最近活跃</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]"></th>
              </tr>
            </thead>
            <tbody>
              {items.length > 0 ? items.map((row) => (
                <tr key={row.api_key_name} className={`cursor-pointer ${selectedApiKeyName === row.api_key_name ? 'bg-[#f2f4ff]' : 'hover:bg-[#fbfcff]'}`} onClick={() => row.api_key_name && onSelectApiKey(row.api_key_name)}>
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">
                    <div className="font-[680] text-[#182033]">{row.api_key_name || 'Unnamed Key'}</div>
                    <div className="mt-[2px] text-[8px] text-[#99a3b3]">{row.last_request_at || '—'}</div>
                  </td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(row.total_cost)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_requests)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{row.primary_model || '—'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] text-[#7b8496]">{row.last_request_at || '—'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px]">
                    <button type="button" onClick={(e) => { e.stopPropagation(); row.api_key_name && onSelectApiKey(row.api_key_name); }} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">查看详情 →</button>
                  </td>
                </tr>
              )) : <tr><td colSpan={7} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">{copy.noApiKeys}</td></tr>}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
