type Props = {
  items: Array<{ label: string; value: string; pct: number; barColor?: string }>;
};

const BAR_COLOR = '#5268f6';

export function BreakdownBar({ items }: Props) {
  if (items.length === 0) {
    return <div className="py-8 text-center text-sm text-[#7b8496]">暂无数据</div>;
  }

  return (
    <div className="space-y-[13px]">
      {items.map((item) => (
        <div key={item.label}>
          <div className="mb-[5px] flex justify-between gap-[10px] text-[10px]">
            <span className="text-[#182033]">{item.label}</span>
            <b className="text-[#182033]">{item.value}</b>
          </div>
          <div className="h-[7px] overflow-hidden rounded-[999px] bg-[#f0f2f6]">
            <span
              className="block h-full rounded-[999px]"
              style={{
                width: `${Math.max(0, Math.min(100, item.pct))}%`,
                background: item.barColor || BAR_COLOR,
              }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}
