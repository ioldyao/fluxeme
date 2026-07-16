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
    <Card className="card-hover">
      <CardContent className="p-5 flex items-center gap-3 press-feedback">
        <div className="p-2 rounded-lg bg-brand/10 text-brand shrink-0">{icon}</div>
        <div className="min-w-0">
          <span className="text-xs text-muted-foreground truncate">{title}</span>
          <span className="text-xl font-semibold mt-0.5 block">
            {loading ? '...' : value}
          </span>
        </div>
      </CardContent>
    </Card>
  );
}
