type TooltipEntry = {
  color?: string;
  name?: string;
  value?: number | string | null;
};

type DashboardChartTooltipProps = {
  active?: boolean;
  payload?: TooltipEntry[];
  label?: string;
};

export function DashboardChartTooltip({ active, payload, label }: DashboardChartTooltipProps) {
  if (!active || !payload?.length) {
    return null;
  }

  return (
    <div className="rounded-lg border bg-popover px-3 py-2 text-xs shadow-md">
      {label && <p className="mb-1 font-medium text-popover-foreground">{label}</p>}
      {payload.map((entry, index) => (
        <div key={`${entry.name ?? 'value'}-${index}`} className="flex items-center gap-2 text-muted-foreground">
          <span className="size-2 rounded-full" style={{ background: entry.color }} />
          <span>{entry.name}</span>
          <span className="ml-auto font-mono font-medium text-popover-foreground">
            {typeof entry.value === 'number' ? entry.value.toLocaleString() : entry.value}
          </span>
        </div>
      ))}
    </div>
  );
}
