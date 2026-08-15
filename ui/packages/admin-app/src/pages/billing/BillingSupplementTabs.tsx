import type { BillingCopy, BillingDetailTab } from './types';

type MonthSummary = {
  month: string;
  total_cost: number;
  total_requests: number;
  total_tokens: number;
};

type ModelRow = {
  model: string;
  cost: number;
  percentage: number;
};

type TokenRow = {
  token_type: string;
  total_tokens: number;
  total_cost: number;
  percentage: number;
};

type Props = {
  copy: BillingCopy;
  tab: BillingDetailTab;
  onTabChange: (tab: BillingDetailTab) => void;
  monthRows: MonthSummary[];
  modelRows: ModelRow[];
  tokenRows: TokenRow[];
  fmtCurrency: (amount: number) => string;
  compactNumber: (value: number) => string;
};

function buildTrendPath(rows: MonthSummary[]) {
  if (rows.length === 0) return '';
  const values = rows.map((row) => row.total_cost);
  const max = Math.max(...values, 1);
  return rows.map((row, index) => {
    const x = rows.length === 1 ? 350 : (index / (rows.length - 1)) * 700;
    const y = 122 - ((row.total_cost / max) * 80);
    return `${index === 0 ? 'M' : 'L'}${x},${y}`;
  }).join(' ');
}

function buildAreaPath(rows: MonthSummary[]) {
  const line = buildTrendPath(rows);
  if (!line) return '';
  return `${line} L700,150 L0,150 Z`;
}

export function BillingSupplementTabs({ copy, tab, onTabChange, monthRows, modelRows, tokenRows, fmtCurrency, compactNumber }: Props) {
  const trendRows = [...monthRows].sort((a, b) => a.month.localeCompare(b.month)).slice(-6);
  const trendLine = buildTrendPath(trendRows);
  const trendArea = buildAreaPath(trendRows);
  const topModels = modelRows.slice(0, 3);
  const topTokens = tokenRows.slice(0, 3);

  const panelClass = (panel: BillingDetailTab) => tab === panel ? 'ring-1 ring-[#dfe5ff] bg-[#fbfcff]' : '';

  return (
    <section className="overflow-hidden rounded-[13px] border border-[#e6ebf2] bg-white">
      <div className="flex flex-col items-start justify-between gap-3 border-b border-[#eef2f6] px-[14px] py-[12px] xl:flex-row xl:items-center">
        <div>
          <div className="text-[12px] font-[700] text-[#182033]">{copy.supplementTitle}</div>
          <div className="mt-[3px] text-[9px] text-[#7c8798]">{copy.supplementSub}</div>
        </div>
        <div className="flex gap-[5px]">
          {([
            ['trend', copy.tabTrend],
            ['model', copy.tabModelCost],
            ['token', copy.tabTokenCost],
          ] as Array<[BillingDetailTab, string]>).map(([nextTab, label]) => (
            <button
              key={nextTab}
              type="button"
              onClick={() => onTabChange(nextTab)}
              className={`rounded-[6px] px-[8px] py-[5px] text-[9px] ${tab === nextTab ? 'bg-[#eef1ff] font-[650] text-[#5268ff]' : 'bg-[#f4f6f9] text-[#697486]'}`}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      <div className="grid gap-4 px-[14px] py-[14px] xl:grid-cols-[1.2fr_.8fr_.8fr]">
        <div className={`rounded-[10px] p-2 ${panelClass('trend')}`}>
          <div className="h-[140px] relative bg-[linear-gradient(to_bottom,transparent_32.8%,#eef1f5_33%,transparent_33.3%),linear-gradient(to_bottom,transparent_65.8%,#eef1f5_66%,transparent_66.3%)]">
            {trendRows.length > 0 ? (
              <svg viewBox="0 0 700 150" preserveAspectRatio="none" className="h-full w-full">
                <defs>
                  <linearGradient id="billing-area" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#5268ff" stopOpacity="0.18" />
                    <stop offset="100%" stopColor="#5268ff" stopOpacity="0" />
                  </linearGradient>
                </defs>
                <path d={trendArea} fill="url(#billing-area)" />
                <path d={trendLine} fill="none" stroke="#5268ff" strokeWidth="3" />
              </svg>
            ) : (
              <div className="grid h-full place-items-center text-sm text-[#7b8496]">{copy.noTrendData}</div>
            )}
          </div>
        </div>

        <div className={`flex flex-col gap-[10px] rounded-[10px] p-2 ${panelClass('model')}`}>
          {topModels.length > 0 ? topModels.map((row) => (
            <div key={row.model} className="text-[9px]">
              <div className="mb-[5px] flex justify-between gap-3">
                <span className="truncate text-[#182033]">{row.model}</span>
                <b className="text-[#182033]">{row.percentage.toFixed(0)}%</b>
              </div>
              <div className="h-[6px] overflow-hidden rounded-[99px] bg-[#f0f2f6]"><span className="block h-full bg-[#5268ff]" style={{ width: `${Math.max(0, Math.min(100, row.percentage))}%` }} /></div>
              <div className="mt-[4px] text-[8px] text-[#8d97a7]">{fmtCurrency(row.cost)}</div>
            </div>
          )) : <div className="py-8 text-center text-sm text-[#7b8496]">{copy.noModelBreakdown}</div>}
        </div>

        <div className={`flex flex-col gap-[10px] rounded-[10px] p-2 ${panelClass('token')}`}>
          {topTokens.length > 0 ? topTokens.map((row) => (
            <div key={row.token_type} className="text-[9px]">
              <div className="mb-[5px] flex justify-between gap-3">
                <span className="truncate text-[#182033]">{row.token_type}</span>
                <b className="text-[#182033]">{fmtCurrency(row.total_cost)}</b>
              </div>
              <div className="h-[6px] overflow-hidden rounded-[99px] bg-[#f0f2f6]"><span className="block h-full bg-[#5268ff]" style={{ width: `${Math.max(0, Math.min(100, row.percentage))}%` }} /></div>
              <div className="mt-[4px] text-[8px] text-[#8d97a7]">{compactNumber(row.total_tokens)} tokens</div>
            </div>
          )) : <div className="py-8 text-center text-sm text-[#7b8496]">{copy.noTokenBreakdown}</div>}
        </div>
      </div>
    </section>
  );
}
