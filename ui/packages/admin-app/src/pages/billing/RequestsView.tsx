import type { BillingCopy } from './types';
import type { UsageRecord } from '@fluxeme/shared/src/types';

type Props = {
  copy: BillingCopy;
  items: UsageRecord[];
  onRequestDetail: (requestId: string) => void;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

function requestCost(r: UsageRecord) {
  const uncached = Math.max(r.prompt_tokens - (r.cache_hit_input_tokens || 0), 0);
  return (
    (uncached / 1_000_000) * (r.prompt_price || 0) +
    ((r.cache_hit_input_tokens || 0) / 1_000_000) * (r.cache_read_price || 0) +
    (r.completion_tokens / 1_000_000) * (r.completion_price || 0)
  );
}

export function RequestsView({ copy, items, onRequestDetail, fmtCurrency, compactNumber }: Props) {
  return (
    <div className="space-y-[14px]">
      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div>
            <div className="text-[13px] font-[700] text-[#182033]">请求级账单明细</div>
            <div className="mt-[3px] text-[9px] text-[#778296]">最终事实层：每一分钱都能追溯到请求</div>
          </div>
          <span className="rounded-full bg-[#f1f3f6] px-[7px] py-[4px] text-[9px] font-[650] text-[#788393]">事实明细</span>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[1100px] border-collapse">
            <thead>
              <tr>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">请求 ID</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">时间</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">用户</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">API Key</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">模型</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">渠道</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">输入</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">缓存</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">输出</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]">费用</th>
                <th className="whitespace-nowrap border-b border-[#e6ebf2] bg-[#fafbfd] px-[12px] py-[9px] text-left text-[9px] font-[650] text-[#8994a5]"></th>
              </tr>
            </thead>
            <tbody>
              {items.length > 0 ? items.map((r) => (
                <tr key={r.request_id} className="hover:bg-[#fbfcff]">
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] font-[680] text-[#182033]">{r.request_id.slice(0, 12)}...</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.timestamp}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.user_name}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.api_key_name || '—'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.model}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{r.channel_id}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.prompt_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.cache_hit_input_tokens || 0)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(r.completion_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(requestCost(r))}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px]">
                    <button type="button" onClick={() => onRequestDetail(r.request_id)} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">追溯 →</button>
                  </td>
                </tr>
              )) : <tr><td colSpan={11} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">{copy.noRequests}</td></tr>}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
