import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowUpRight } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/EmptyState';
import type { UsageRecord } from '@/types';

type DashboardRequestLogsCardProps = {
  records: UsageRecord[];
  isLoading: boolean;
  isError: boolean;
};

function formatTimestamp(value: string) {
  return new Date(value).toLocaleString();
}

export function DashboardRequestLogsCard({ records, isLoading, isError }: DashboardRequestLogsCardProps) {
  const { t } = useTranslation();

  return (
    <Card className="card-hover">
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold leading-none">{t('dash.requestLogs')}</h3>
          <CardDescription>{t('dash.requestLogsSub')}</CardDescription>
        </div>
        <Button asChild variant="ghost" size="sm">
          <Link to="/usage">
            {t('dash.viewAllUsage')}
            <ArrowUpRight className="ml-1 size-4" />
          </Link>
        </Button>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="space-y-3">
            {Array.from({ length: 6 }).map((_, index) => (
              <div key={index} className="h-12 animate-pulse rounded-lg bg-muted/60" />
            ))}
          </div>
        ) : isError ? (
          <EmptyState message={t('err.loadFailed')} />
        ) : records.length === 0 ? (
          <EmptyState message={t('dash.noRecentUsage')} />
        ) : (
          <div className="overflow-auto">
            <table className="min-w-full border-collapse text-sm">
              <thead>
                <tr className="border-b bg-muted/20 text-left text-xs text-muted-foreground">
                  <th className="px-4 py-3 font-medium">{t('table.time')}</th>
                  <th className="px-4 py-3 font-medium">{t('table.status')}</th>
                  <th className="px-4 py-3 font-medium">{t('table.model')}</th>
                  <th className="px-4 py-3 font-medium">ID</th>
                  <th className="px-4 py-3 font-medium">{t('table.tokens')}</th>
                  <th className="px-4 py-3 font-medium">{t('table.latency')}</th>
                  <th className="px-4 py-3 font-medium">{t('table.key')}</th>
                </tr>
              </thead>
              <tbody>
                {records.map((record) => (
                  <tr key={record.request_id} className="border-b last:border-b-0">
                    <td className="px-4 py-3 text-muted-foreground">{formatTimestamp(record.timestamp)}</td>
                    <td className="px-4 py-3">
                      <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${record.success ? 'bg-emerald-500/10 text-emerald-700' : 'bg-red-500/10 text-red-700'}`}>
                        <span className={`size-1.5 rounded-full ${record.success ? 'bg-emerald-500' : 'bg-red-500'}`} aria-hidden="true" />
                        {record.success ? t('usage.success') : t('usage.failure')}
                      </span>
                    </td>
                    <td className="px-4 py-3 font-medium text-foreground">{record.model}</td>
                    <td className="px-4 py-3 font-mono text-xs text-muted-foreground">{record.request_id}</td>
                    <td className="px-4 py-3">{record.total_tokens.toLocaleString()}</td>
                    <td className="px-4 py-3">{record.latency_ms}ms</td>
                    <td className="px-4 py-3 font-mono text-xs text-muted-foreground">{record.api_key_name ?? '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
