import type { BillingCopy, BillingView } from './types';
import type { UsageRecord } from '@fluxeme/shared/src/types';
import { EntityHead } from './EntityHead';
import { CostCalcTable, type CalcRow } from './CostCalcTable';

type Props = {
  copy: BillingCopy;
  onViewChange: (view: BillingView) => void;
  request: UsageRecord;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

export function RequestDetailView({ onViewChange, request, fmtCurrency }: Props) {
  const uncached = Math.max(request.prompt_tokens - (request.cache_hit_input_tokens || 0), 0);
  const cacheHits = request.cache_hit_input_tokens || 0;

  const calcRows: CalcRow[] = [
    { label: '未缓存输入', tokens: uncached, rate: request.prompt_price || 0, cost: (uncached / 1_000_000) * (request.prompt_price || 0) },
    { label: '缓存命中输入', tokens: cacheHits, rate: request.cache_read_price || 0, cost: (cacheHits / 1_000_000) * (request.cache_read_price || 0) },
    { label: '输出', tokens: request.completion_tokens, rate: request.completion_price || 0, cost: (request.completion_tokens / 1_000_000) * (request.completion_price || 0) },
  ];
  const totalCost = calcRows.reduce((s, r) => s + r.cost, 0);

  return (
    <div className="space-y-[14px]">
      <div className="flex items-center gap-[6px] text-[10px] text-[#8792a3]">
        <button type="button" onClick={() => onViewChange('requests')} className="border-0 bg-transparent p-0 text-[10px] text-[#5268f6]">请求明细</button>
        <span>/</span><b className="text-[#283448]">{request.request_id}</b>
      </div>

      <EntityHead avatar="R" name={request.request_id} meta="请求级费用追溯" extra={
        <span className="rounded-full bg-[#eaf8f2] px-[7px] py-[4px] text-[9px] font-[650] text-[#138d61]">{request.success ? (request.status_code ? `${request.status_code} ${request.api_format}` : '200 OK') : 'Error'}</span>
      } />

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div><div className="text-[13px] font-[700] text-[#182033]">请求上下文</div><div className="mt-[3px] text-[9px] text-[#778296]">谁发起、谁付费、走什么模型和渠道</div></div>
        </div>
        <div className="grid grid-cols-3 gap-[10px] px-[14px] py-[14px]">
          {[
            ['时间', request.timestamp],
            ['用户', request.user_name],
            ['API Key', request.api_key_name || '—'],
            ['模型', request.model],
            ['渠道', request.channel_id],
            ['状态', request.success ? '成功' : '失败'],
          ].map(([k, v]) => (
            <div key={k as string} className="rounded-[10px] border border-[#e6ebf2] px-[11px] py-[11px]">
              <small className="mb-[5px] block text-[9px] text-[#778296]">{k as string}</small>
              <b className="text-[13px] text-[#182033]">{v as string}</b>
            </div>
          ))}
        </div>
      </div>

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div><div className="text-[13px] font-[700] text-[#182033]">Token 与计费计算</div><div className="mt-[3px] text-[9px] text-[#778296]">把最终费用逐项算清楚</div></div>
        </div>
        <div className="py-[14px]">
          <CostCalcTable rows={calcRows} total={totalCost} />
        </div>
      </div>

      <div className="grid gap-[14px] xl:grid-cols-2">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">扣费记录</div></div>
          </div>
          <div className="px-[14px] py-[14px]">
            <div className="flex items-center justify-between gap-[12px] border-b border-[#eef1f5] py-[10px]">
              <div><b className="block text-[10px]">扣费 ID</b><small className="mt-[3px] block text-[9px] text-[#778296]">扣费记录 ID</small></div>
              <div className="text-right"><b className="block text-[10px]">{fmtCurrency(totalCost)}</b><small className="mt-[3px] block text-[9px] text-[#778296]">已扣费</small></div>
            </div>
            <div className="flex items-center justify-between gap-[12px] py-[10px]">
              <div><b className="block text-[10px]">倍率</b><small className="mt-[3px] block text-[9px] text-[#778296]">Pricing multiplier</small></div>
              <div className="text-right"><b className="block text-[10px]">1.0x</b><small className="mt-[3px] block text-[9px] text-[#778296]">无额外倍率</small></div>
            </div>
          </div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">追溯信息</div></div>
          </div>
          <div className="px-[14px] py-[14px]">
            <div className="flex items-center justify-between gap-[12px] border-b border-[#eef1f5] py-[10px]">
              <div><b className="block text-[10px]">计费规则</b><small className="mt-[3px] block text-[9px] text-[#778296]">{request.model}</small></div>
              <div className="text-right"><b className="block text-[10px]">命中</b><small className="mt-[3px] block text-[9px] text-[#778296]">版本已锁定</small></div>
            </div>
            <div className="flex items-center justify-between gap-[12px] py-[10px]">
              <div><b className="block text-[10px]">请求状态</b><small className="mt-[3px] block text-[9px] text-[#778296]">{request.timestamp}</small></div>
              <div className="text-right"><b className="block text-[10px]">{request.success ? '成功' : '失败'}</b><small className="mt-[3px] block text-[9px] text-[#778296]">可复核</small></div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
