import { StatCard } from '@/components/StatCard';
import type { DashboardStatItem } from './dashboardViewModel';

type DashboardStatsGridProps = {
  items: DashboardStatItem[];
  isLoading: boolean;
};

export function DashboardStatsGrid({ items, isLoading }: DashboardStatsGridProps) {
  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-3 lg:grid-cols-5">
      {items.map((item, index) => (
        <div
          key={item.title}
          style={{ animation: `fade-in 0.35s var(--ease-out) ${index * 0.06}s both` }}
        >
          <StatCard
            title={item.title}
            value={item.value.toLocaleString()}
            icon={item.icon}
            loading={isLoading}
          />
        </div>
      ))}
    </div>
  );
}
