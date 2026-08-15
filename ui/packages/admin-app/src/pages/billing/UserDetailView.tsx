import type { BillingCopy, BillingView } from './types';
import type { PeriodSummary } from '@fluxeme/shared/src/api/billing';
import type { AdminBillingUserSpendRow, AdminBillingUserApiKeyCostRow, AdminBillingTrendPoint } from '@fluxeme/shared/src/types';
import { EntityHead } from './EntityHead';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';
import { TrendChart } from './TrendChart';
import { BreakdownBar } from './BreakdownBar';

type Props = {
  copy: BillingCopy;
  onViewChange: (view: BillingView) => void;
  onKeyDetail: (keyName: string) => void;
  user: AdminBillingUserSpendRow;
  scopedPeriod?: PeriodSummary;
  trendData: AdminBillingTrendPoint[];
  apiKeys: AdminBillingUserApiKeyCostRow[];
  apiKeyTotal: number;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
};

export function UserDetailView({
  copy,
  onViewChange,
  onKeyDetail,
  user,
  scopedPeriod,
  trendData,
  apiKeys,
  apiKeyTotal,
  fmtCurrency,
  compactNumber,
}: Props) {
  const inputRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'input');
  const cacheRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'cache_hit');
  const outputRow = scopedPeriod?.token_cost_breakdown?.find(r => r.token_type === 'output');

  const kpis: KpiItem[] = [
    { label: '本期消费', value: scopedPeriod ? fmtCurrency(scopedPeriod.total_cost) : '—' },
    { label: '请求数', value: scopedPeriod ? compactNumber(scopedPeriod.total_requests) : '—' },
    { label: 'Token 使用量', value: scopedPeriod ? compactNumber(scopedPeriod.total_tokens) : '—' },
    { label: 'API Key 数量', value: compactNumber(apiKeyTotal) },
  ];

  const breakdownItems = [
    { label: '输入费用', value: `${fmtCurrency(inputRow?.total_cost ?? 0)} · ${(inputRow?.percentage ?? 0).toFixed(0)}%`, pct: inputRow?.percentage ?? 0 },
    { label: '缓存命中费用', value: `${fmtCurrency(cacheRow?.total_cost ?? 0)} · ${(cacheRow?.percentage ?? 0).toFixed(0)}%`, pct: cacheRow?.percentage ?? 0 },
    { label: '输出费用', value: `${fmtCurrency(outputRow?.total_cost ?? 0)} · ${(outputRow?.percentage ?? 0).toFixed(0)}%`, pct: outputRow?.percentage ?? 0 },
  ];

  return (
    <div className="space-y-[14px]">
      <div className="flex items-center gap-[6px] text-[10px] text-[#8792a3]">
        <button type="button" onClick={() => onViewChange('users')} className="border-0 bg-transparent p-0 text-[10px] text-[#5268f6]">用户账单</button>
        <span>/</span>
        <b className="text-[#283448]">{user.user_name}</b>
      </div>

      <EntityHead avatar={user.user_name.charAt(0).toUpperCase()} name={user.user_name} meta={`${user.user_id} · 个人用户账单`} />

      <BillingKpiRow items={kpis} />

      <div className="grid gap-[14px] xl:grid-cols-[1.3fr_.7fr]">
        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">每日消费趋势</div><div className="mt-[3px] text-[9px] text-[#778296]">看这个用户本期消费何时上涨</div></div>
            <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">用户维度</span>
          </div>
          <div className="h-[235px] px-[16px] py-[14px]"><TrendChart data={trendData} emptyLabel={copy.noTrendData} /></div>
        </div>

        <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
          <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
            <div><div className="text-[13px] font-[700] text-[#182033]">费用构成</div><div className="mt-[3px] text-[9px] text-[#778296]">本用户费用怎么形成</div></div>
          </div>
          <div className="px-[14px] py-[14px]"><BreakdownBar items={breakdownItems} /></div>
        </div>
      </div>

      <div className="overflow-hidden rounded-[12px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
        <div className="flex items-center justify-between gap-[10px] border-b border-[#eef1f5] px-[15px] py-[13px]">
          <div><div className="text-[13px] font-[700] text-[#182033]">API Key 账单</div><div className="mt-[3px] text-[9px] text-[#778296]">该用户自己的 API Key 消费明细</div></div>
          <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{apiKeys.length} 个 Key</span>
        </div>
        <div className="overflow-auto">
          <table className="w-full min-w-[800px] border-collapse">
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
              {apiKeys.length > 0 ? apiKeys.map((row) => (
                <tr key={row.api_key_name} className="hover:bg-[#fbfcff]">
                  <td className="border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]"><div className="font-[680] text-[#182033]">{row.api_key_name || 'Unnamed Key'}</div><div className="mt-[2px] text-[8px] text-[#99a3b3]">个人用户</div></td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] font-[720] text-[#182033]">{fmtCurrency(row.total_cost)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_requests)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{compactNumber(row.total_tokens)}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px]">{row.primary_model || '—'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px] text-[10px] text-[#7b8496]">{row.last_request_at || '—'}</td>
                  <td className="whitespace-nowrap border-b border-[#f0f2f5] px-[12px] py-[12px]">{row.api_key_name ? <button type="button" onClick={() => onKeyDetail(row.api_key_name!)} className="border-0 bg-transparent p-0 text-[10px] font-[650] text-[#5268f6]">查看详情 →</button> : null}</td>
                </tr>
              )) : (
                <tr><td colSpan={7} className="border-b border-[#f0f2f5] px-[12px] py-[17px] text-center text-[10px] text-[#8a95a6]">{copy.noApiKeys}</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
