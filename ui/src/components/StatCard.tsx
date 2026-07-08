import type { ReactNode } from 'react';
import { Card, CardContent } from '@/components/ui/card';

interface StatCardProps {
  title: string;
  value: number | string;
  icon: ReactNode;
  loading?: boolean;
}

export function StatCard({ title, value, icon, loading }: StatCardProps) {
  return (
    <Card>
      <CardContent className="p-4 flex items-center gap-3">
        <div className="text-brand">{icon}</div>
        <div className="flex flex-col">
          <span className="text-xs text-muted-foreground">{title}</span>
          <span className="text-xl font-semibold">
            {loading ? '...' : value}
          </span>
        </div>
      </CardContent>
    </Card>
  );
}
