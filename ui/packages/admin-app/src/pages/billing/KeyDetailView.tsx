import type { BillingCopy, BillingView } from './types';
import type { AdminBillingApiKeyDetailResponse, UsageRecord } from '@fluxeme/shared/src/types';
import { EntityHead } from './EntityHead';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';

type Props = {
  copy: BillingCopy;
  onViewChange: (view: BillingView) => void;
  detail: AdminBillingApiKeyDetailResponse;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
  onRequestDetail: (requestId: string) => void;
};

function requestCost(r: UsageRecord) {
  const uncached = Math.max(r.prompt_tokens - (r.cache_hit_input_tokens || 0), 0);
  return (
    (uncached / 1_000_000) * (r.prompt_price || 0) +
    ((r.cache_hit_input_tokens || 0) / 1_000_000) * (r.cache_read_price || 0) +
    (r.completion_tokens / 1_000_000) * (r.completion_price || 0)
  );
}

export function KeyDetailView({ copy, onViewChange, detail, fmtCurrency, compactNumber, onRequestDetail }: Props) {
  const totalRequestCost = detail.recent_requests.reduce((s, r) => s + requestCost(r), 0);
  const mainModel = detail.top_models[0]?.model || '—';

  const kpis: KpiItem[] = [
    { label: '本期消费', value: fmtCurrency(totalRequestCost) },
    { label: '请求数', value: compactNumber(detail.total_requests) },
    { label: 'Token', value: compactNumber(detail.total_tokens) },
    { label: '主要模型', value: mainModel },
  ];

  return (
    <div className="space-y-[14px]">
      <div className="flex items-center gap-[6px] text-[10px] text-[#8792a3]">
        <button type="button" onClick={() => onViewChange('keys')} className="border-0 bg-transparent p-0 text-[10px] text-[#5268f6]">API Key</button>
        <span>/</span><b className="text-[#283448]">{detail.api_key_name}</b>
      </div>

      <EntityHead avatar="K" name={detail.api_key_name} meta={`${detail.user_id} · ${detail.team?.team_name ?? '个人用户'}`} />

      <BillingKpiRow items={kpis} />

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div><div className="text-[13px] font-[700] text-[#182033]">该 API Key 请求账单</div><div className="mt-[3px] text-[9px] text-[#778296]">继续追溯到单请求</div></div>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[800px] border-collapse">
            <thead>
              <tr>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求 ID</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">模型</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">输入</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">缓存</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">输出</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">费用</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]"></th>
              </tr>
            </thead>
            <tbody>
              {detail.recent_requests.length > 0 ? detail.recent_requests.map((r) => (
                <tr key={r.request_id} className="hover:bg-[#fbfcff]">
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]"><div className="font-[680] text-[#182033]">{r.request_id.slice(0, 12)}...</div><div className="mt-[2px] text-[8px] text-[#99a3b3]">{r.timestamp}</div></td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.model}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.prompt_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.cache_hit_input_tokens || 0)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.completion_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(requestCost(r))}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px]">
                    <button type="button" onClick={() => onRequestDetail(r.request_id)} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">追溯 →</button>
                  </td>
                </tr>
              )) : <tr><td colSpan={7} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">{copy.noRequests}</td></tr>}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
