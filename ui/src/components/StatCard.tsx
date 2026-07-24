import type { ReactNode } from 'react';
import { Card, CardContent } from '@/components/ui/card';

interface StatCardProps {
  title: string;
  value: number | string;
  icon: ReactNode;
  loading?: boolean;
  subtitle?: string;
}

export function StatCard({ title, value, icon, loading, subtitle }: StatCardProps) {
  return (
    <Card className="card-hover h-full">
      <CardContent className="flex h-full items-center gap-3 p-5 press-feedback">
        <div className="shrink-0 rounded-lg bg-brand/10 p-2 text-brand">{icon}</div>
        <div className="min-w-0 space-y-1" aria-busy={loading || undefined}>
          <span className="block truncate text-xs text-muted-foreground">{title}</span>
          <span className="block text-xl font-semibold leading-none sm:text-2xl">
            {loading ? (
              <>
                <span aria-hidden="true">...</span>
                <span className="sr-only">Loading</span>
              </>
            ) : value}
          </span>
          {subtitle && <span className="block truncate text-xs text-muted-foreground">{subtitle}</span>}
        </div>
      </CardContent>
    </Card>
  );
}
