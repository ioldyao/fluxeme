import type { BillingMetricCard } from './types';

type Props = {
  metrics: BillingMetricCard[];
};

export function BillingScopeHeader({ metrics }: Props) {
  return (
    <section className="grid gap-[10px] md:grid-cols-2 xl:grid-cols-4">
      {metrics.map((card) => (
        <div key={card.label} className="rounded-[12px] border border-[#e6ebf2] bg-white px-[15px] py-[13px]">
          <div className="mb-[7px] text-[10px] text-[#7c8798]">{card.label}</div>
          <div className="text-[19px] font-[750] text-[#182033]">{card.value}</div>
          {card.meta ? <div className="mt-[5px] text-[9px] text-[#98a2b3]">{card.meta}</div> : null}
        </div>
      ))}
    </section>
  );
}
