export interface CalcRow {
  label: string;
  tokens: number;
  rate: number;
  cost: number;
}

type Props = {
  rows: CalcRow[];
  total: number;
  pricingVersion?: string;
};

export function CostCalcTable({ rows, total, pricingVersion }: Props) {
  return (
    <div>
      <div className="grid grid-cols-[1.2fr_.8fr_.8fr_.8fr] gap-[10px] border-b border-[#eef1f5] px-[14px] py-[9px] text-[9px] text-[#778296]">
        <span>项目</span><span>Token</span><span>单价/1M</span><span>费用</span>
      </div>
      {rows.map((row) => (
        <div
          key={row.label}
          className="grid grid-cols-[1.2fr_.8fr_.8fr_.8fr] gap-[10px] border-b border-[#eef1f5] px-[14px] py-[9px] text-[10px]"
        >
          <span className="text-[#182033]">{row.label}</span>
          <span className="text-[#182033]">{row.tokens.toLocaleString('zh-CN')}</span>
          <span className="text-[#182033]">¥{row.rate.toFixed(2)}</span>
          <span className="text-[#182033]">¥{row.cost.toFixed(6)}</span>
        </div>
      ))}
      <div className="flex justify-between px-[14px] pt-[12px] text-[12px]">
        <span>
          最终扣费
          {pricingVersion ? (
            <span className="ml-2 rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{pricingVersion}</span>
          ) : null}
        </span>
        <b className="text-[15px] text-[#182033]">¥{total.toFixed(6)}</b>
      </div>
    </div>
  );
}
