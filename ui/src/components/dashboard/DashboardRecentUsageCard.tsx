import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Clock3, ExternalLink } from 'lucide-react';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/EmptyState';
import { useAuth } from '@/store/auth';
import type { UsageRecord } from '@/types';

type DashboardRecentUsageCardProps = {
  records: UsageRecord[];
  isLoading: boolean;
  isError: boolean;
};

function formatTimestamp(value: string, timezone: string) {
  try {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
      timeZone: timezone || 'UTC',
    }).format(new Date(value));
  } catch {
    return new Date(value).toLocaleString();
  }
}

export function DashboardRecentUsageCard({ records, isLoading, isError }: DashboardRecentUsageCardProps) {
  const { t } = useTranslation();
  const timezone = useAuth((state) => state.timezone);

  return (
    <Card className="card-hover lg:col-span-2">
      <CardHeader className="flex flex-row items-center justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
            <Clock3 className="size-4 text-brand" />
            {t('dash.recentUsage')}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">{t('dash.recentUsageSub')}</p>
        </div>
        <Button asChild variant="ghost" size="sm">
          <Link to="/usage">
            {t('dash.viewAllUsage')}
            <ExternalLink className="ml-1 size-4" />
          </Link>
        </Button>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="space-y-3">
            {Array.from({ length: 5 }).map((_, index) => (
              <div key={index} className="h-14 animate-pulse rounded-lg bg-muted/60" />
            ))}
          </div>
        ) : isError ? (
          <EmptyState message={t('err.loadFailed')} />
        ) : records.length === 0 ? (
          <EmptyState message={t('dash.noRecentUsage')} action={<p className="text-sm text-muted-foreground">{t('dash.startUsing')}</p>} />
        ) : (
          <div className="space-y-2">
            {records.map((record) => (
              <div key={record.request_id} className="flex flex-col gap-3 rounded-lg border bg-muted/20 p-3 sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0 space-y-1">
                  <div className="flex items-center gap-2">
                    <span className={`size-2 shrink-0 rounded-full ${record.success ? 'bg-emerald-500' : 'bg-red-500'}`} aria-hidden="true" />
                    <span className="truncate font-medium text-foreground">{record.model}</span>
                    <span className={`inline-flex shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium ${record.success ? 'bg-emerald-500/10 text-emerald-700' : 'bg-red-500/10 text-red-700'}`}>
                      {record.success ? t('usage.success') : t('usage.failure')}
                    </span>
                  </div>
                  <p className="truncate text-xs text-muted-foreground">{formatTimestamp(record.timestamp, timezone)}</p>
                </div>
                <div className="grid grid-cols-2 gap-3 text-sm sm:min-w-[220px] sm:grid-cols-2">
                  <div>
                    <p className="text-xs text-muted-foreground">{t('table.tokens')}</p>
                    <p className="font-medium">{record.total_tokens.toLocaleString()}</p>
                  </div>
                  <div>
                    <p className="text-xs text-muted-foreground">{t('table.latency')}</p>
                    <p className="font-medium">{record.latency_ms}ms</p>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
