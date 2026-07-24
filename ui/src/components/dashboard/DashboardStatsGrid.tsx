import { StatCard } from '@/components/StatCard';
import type { DashboardStatItem } from './dashboardViewModel';

type DashboardStatsGridProps = {
  items: DashboardStatItem[];
  isLoading: boolean;
};

export function DashboardStatsGrid({ items, isLoading }: DashboardStatsGridProps) {
  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
      {items.map((item, index) => (
        <div
          key={item.title}
          style={{ animation: `fade-in 0.35s var(--ease-out) ${index * 0.06}s both` }}
        >
          <StatCard
            title={item.title}
            value={item.value}
            subtitle={item.subtitle}
            icon={item.icon}
            loading={isLoading}
          />
        </div>
      ))}
    </div>
  );
}
