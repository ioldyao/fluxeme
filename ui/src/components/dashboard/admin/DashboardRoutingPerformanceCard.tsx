import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { ArrowUpRight } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';
import { Button } from '@/components/ui/button';

type RoutingPerformanceRow = {
  channelId: string;
  channelName: string;
  routeRole: string;
  share: number;
  requests: number;
  avgLatency: number;
};

type DashboardRoutingPerformanceCardProps = {
  rows: RoutingPerformanceRow[];
  isLoading: boolean;
  isError: boolean;
};

function formatLatency(value: number) {
  return value >= 1000 ? `${(value / 1000).toFixed(2)}s` : `${value.toFixed(0)}ms`;
}

export function DashboardRoutingPerformanceCard({ rows, isLoading, isError }: DashboardRoutingPerformanceCardProps) {
  const { t } = useTranslation();

  return (
    <Card className="card-hover">
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold leading-none">{t('dash.routingPerformance')}</h3>
          <CardDescription>{t('dash.routingPerformanceSub')}</CardDescription>
        </div>
        <Button asChild variant="ghost" size="sm">
          <Link to="/routing-history">
            {t('dash.viewRouting')}
            <ArrowUpRight className="ml-1 size-4" />
          </Link>
        </Button>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="space-y-3">
            {Array.from({ length: 3 }).map((_, index) => (
              <div key={index} className="h-16 animate-pulse rounded-lg bg-muted/60" />
            ))}
          </div>
        ) : isError ? (
          <p className="py-8 text-center text-sm text-destructive">{t('err.loadFailed')}</p>
        ) : rows.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
        ) : (
          <div className="space-y-3">
            {rows.map((row) => (
              <div key={row.channelId} className="rounded-lg border bg-muted/20 p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="size-2 shrink-0 rounded-full bg-brand" />
                      <span className="truncate font-medium text-foreground">{row.channelName}</span>
                    </div>
                    <p className="mt-1 text-xs text-muted-foreground">{row.routeRole}</p>
                  </div>
                  <div className="text-right text-sm">
                    <div className="font-semibold">{row.share.toFixed(1)}%</div>
                    <div className="text-xs text-muted-foreground">{row.requests.toLocaleString()} {t('dash.requestsUnit')}</div>
                  </div>
                </div>
                <div className="mt-3 h-2 overflow-hidden rounded-full bg-muted">
                  <div className="h-full rounded-full bg-brand transition-all" style={{ width: `${Math.max(6, row.share)}%` }} />
                </div>
                <div className="mt-2 text-xs text-muted-foreground">{t('dash.avgLatency')}: {formatLatency(row.avgLatency)}</div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
