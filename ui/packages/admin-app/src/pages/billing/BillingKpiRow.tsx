
export interface KpiItem {
  label: string;
  value: string;
  meta?: string;
  metaColor?: 'up' | 'down' | 'normal';
}

type Props = {
  items: KpiItem[];
};

export function BillingKpiRow({ items }: Props) {
  const metaClass = (color?: 'up' | 'down' | 'normal') => {
    if (color === 'up') return 'text-[#d94f60]';
    if (color === 'down') return 'text-[#138d61]';
    return 'text-[#99a3b3]';
  };

  return (
    <div className="grid grid-cols-2 gap-[10px] xl:grid-cols-4">
      {items.map((item) => (
        <div key={item.label} className="rounded-[11px] border border-[#e6ebf2] bg-white px-[15px] py-[14px]">
          <div className="mb-[7px] text-[10px] text-[#778296]">{item.label}</div>
          <div className="text-[20px] font-[760] text-[#182033]">{item.value}</div>
          {item.meta ? (
            <div className={`mt-[5px] text-[9px] ${metaClass(item.metaColor)}`}>{item.meta}</div>
          ) : null}
        </div>
      ))}
    </div>
  );
}
