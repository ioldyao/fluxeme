import { useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { BarChart3, RefreshCw, Table2 } from 'lucide-react';
import { Button } from '../ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { DashboardChartTooltip } from '../dashboard/DashboardChartTooltip';
import type { UsageAnalyticsResponse } from '../../api/usage';
import {
  formatCompactNumber,
  topUsageModels,
  toUsageChartBuckets,
  USAGE_CHART_COLORS,
} from '../../lib/usage-chart-data';

type UsageAnalyticsChartsProps = {
  data?: UsageAnalyticsResponse;
  isLoading: boolean;
  isFetching?: boolean;
  isError: boolean;
  onRetry: () => void;
};

type ChartCardProps = {
  title: string;
  description?: string;
  children: ReactNode;
};

function ChartCard({ title, description, children }: ChartCardProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{title}</CardTitle>
        {description && <p className="text-sm text-muted-foreground">{description}</p>}
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

function ChartSkeleton() {
  const { t } = useTranslation();
  return <div className="h-[280px] animate-pulse rounded-lg border bg-muted/20" aria-label={t('common.loading')} />;
}

function AnalyticsTable({ data }: { data: UsageAnalyticsResponse }) {
  const { t } = useTranslation();
  const buckets = toUsageChartBuckets(data.buckets, data.days);
  return (
    <details className="rounded-lg border bg-muted/20 p-3 text-sm">
      <summary className="flex cursor-pointer list-none items-center gap-2 font-medium">
        <Table2 className="size-4" />
        {t('usage.chartViewTable')}
      </summary>
      <div className="mt-3 overflow-x-auto">
        <table className="w-full min-w-[640px] text-xs">
          <thead>
            <tr className="border-b text-left text-muted-foreground">
              <th className="px-2 py-2">{t('table.date')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartRequests')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartSuccessful')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartFailed')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartInputTokens')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartCacheTokens')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartOutputTokens')}</th>
              <th className="px-2 py-2 text-right">{t('usage.chartSuccessRate')}</th>
            </tr>
          </thead>
          <tbody>
            {buckets.map((bucket) => (
              <tr key={bucket.date} className="border-b last:border-0">
                <td className="px-2 py-2">{bucket.date}</td>
                <td className="px-2 py-2 text-right">{bucket.requests.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.succeeded.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.failed.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.input_tokens.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.cache_read_tokens.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.output_tokens.toLocaleString()}</td>
                <td className="px-2 py-2 text-right">{bucket.success_rate === null ? '—' : `${bucket.success_rate.toFixed(1)}%`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </details>
  );
}

export function UsageAnalyticsCharts({ data, isLoading, isFetching, isError, onRetry }: UsageAnalyticsChartsProps) {
  const { t } = useTranslation();
  const [modelMetric, setModelMetric] = useState<'tokens' | 'requests'>('tokens');
  const buckets = useMemo(() => toUsageChartBuckets(data?.buckets ?? [], data?.days), [data?.buckets, data?.days]);
  const models = useMemo(() => topUsageModels(data?.models ?? [], 8, t('usage.chartOther')), [data?.models, t]);

  if (isLoading && !data) {
    return <div className="space-y-4"><ChartSkeleton /><ChartSkeleton /></div>;
  }
  if (isError && !data) {
    return (
      <Card>
        <CardContent className="flex min-h-56 flex-col items-center justify-center gap-3 text-center">
          <p className="text-sm text-destructive">{t('err.loadFailed')}</p>
          <Button variant="outline" onClick={onRetry}><RefreshCw className="mr-1 size-4" />{t('common.refresh')}</Button>
        </CardContent>
      </Card>
    );
  }
  if (!data || data.totals.requests === 0) {
    return <Card><CardContent className="p-8 text-center text-sm text-muted-foreground">{t('usage.chartNoData')}</CardContent></Card>;
  }

  const tokenTotal = data.totals.total_tokens;
  const successRate = data.totals.requests > 0 ? (data.totals.succeeded / data.totals.requests) * 100 : null;
  const summary = [
    [t('usage.chartRequests'), data.totals.requests.toLocaleString()],
    [t('usage.chartSuccessRate'), successRate === null ? '—' : `${successRate.toFixed(1)}%`],
    [t('usage.chartTokens'), formatCompactNumber(tokenTotal)],
    [t('usage.chartAverageLatency'), data.totals.requests > 0 ? `${Math.round(data.totals.latency_ms / data.totals.requests)}ms` : '—'],
  ];

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="grid flex-1 grid-cols-2 gap-2 md:grid-cols-4">
          {summary.map(([label, value]) => (
            <div key={label} className="rounded-lg border bg-card px-3 py-2">
              <div className="text-[11px] uppercase tracking-wide text-muted-foreground">{label}</div>
              <div className="mt-1 text-lg font-semibold">{value}</div>
            </div>
          ))}
        </div>
        {isFetching && <RefreshCw className="size-4 animate-spin text-muted-foreground" aria-label={t('usage.chartRefreshing')} />}
      </div>

      {isError && <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{t('usage.chartShowingPrevious')}</p>}

      <ChartCard title={t('usage.chartOutcomes')} description={t('usage.chartOutcomesDescription')}>
        <ResponsiveContainer width="100%" height={260}>
          <BarChart data={buckets} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
            <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
            <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} allowDecimals={false} />
            <Tooltip content={<DashboardChartTooltip />} />
            <Legend />
            <Bar dataKey="succeeded" stackId="outcomes" fill={USAGE_CHART_COLORS[1]} name={t('usage.chartSuccessful')} />
            <Bar dataKey="failed" stackId="outcomes" fill="var(--destructive)" name={t('usage.chartFailed')} radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </ChartCard>

      <ChartCard title={t('usage.chartComposition')} description={t('usage.chartCompositionDescription')}>
        <ResponsiveContainer width="100%" height={260}>
          <BarChart data={buckets} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
            <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
            <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} tickFormatter={formatCompactNumber} />
            <Tooltip content={<DashboardChartTooltip />} />
            <Legend />
            <Bar dataKey="input_tokens" stackId="tokens" fill={USAGE_CHART_COLORS[0]} name={t('usage.chartInputTokens')} />
            <Bar dataKey="cache_read_tokens" stackId="tokens" fill={USAGE_CHART_COLORS[4]} name={t('usage.chartCacheTokens')} />
            <Bar dataKey="output_tokens" stackId="tokens" fill={USAGE_CHART_COLORS[2]} name={t('usage.chartOutputTokens')} radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </ChartCard>

      <ChartCard title={t('usage.chartRate')} description={t('usage.chartRateDescription')}>
        <ResponsiveContainer width="100%" height={220}>
          <LineChart data={buckets} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
            <XAxis dataKey="date" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} />
            <YAxis domain={[0, 100]} unit="%" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }} width={42} />
            <Tooltip content={<DashboardChartTooltip formatter={(value) => value == null ? '—' : `${Number(value).toFixed(1)}%`} />} />
            <Line connectNulls={false} type="monotone" dataKey="success_rate" stroke={USAGE_CHART_COLORS[1]} strokeWidth={2.5} dot={{ r: 3 }} name={t('usage.chartSuccessRate')} />
          </LineChart>
        </ResponsiveContainer>
      </ChartCard>

      <ChartCard title={t('usage.chartModels')} description={t('usage.chartModelsDescription')}>
        <div className="mb-3 flex items-center gap-2">
          <Button variant={modelMetric === 'tokens' ? 'default' : 'outline'} size="sm" onClick={() => setModelMetric('tokens')}><BarChart3 className="mr-1 size-3.5" />Tokens</Button>
          <Button variant={modelMetric === 'requests' ? 'default' : 'outline'} size="sm" onClick={() => setModelMetric('requests')}>{t('usage.chartRequests')}</Button>
        </div>
        <ResponsiveContainer width="100%" height={280}>
          <BarChart data={models} margin={{ top: 8, right: 8, bottom: 0, left: -12 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
            <XAxis dataKey="model" tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} interval={0} angle={models.length > 5 ? -25 : 0} textAnchor={models.length > 5 ? 'end' : 'middle'} height={models.length > 5 ? 58 : 30} />
            <YAxis tickLine={false} axisLine={false} tick={{ fill: 'var(--muted-foreground)', fontSize: 11 }} tickFormatter={formatCompactNumber} />
            <Tooltip content={<DashboardChartTooltip />} />
            <Legend />
            {modelMetric === 'tokens' ? (
              <>
                <Bar dataKey="input_tokens" stackId="model" fill={USAGE_CHART_COLORS[0]} name={t('usage.chartInputTokens')} />
                <Bar dataKey="cache_read_tokens" stackId="model" fill={USAGE_CHART_COLORS[4]} name={t('usage.chartCacheTokens')} />
                <Bar dataKey="output_tokens" stackId="model" fill={USAGE_CHART_COLORS[2]} name={t('usage.chartOutputTokens')} radius={[4, 4, 0, 0]} />
              </>
            ) : (
              <>
                <Bar dataKey="succeeded" stackId="model-status" fill={USAGE_CHART_COLORS[1]} name={t('usage.chartSuccessful')} />
                <Bar dataKey="failed" stackId="model-status" fill="var(--destructive)" name={t('usage.chartFailed')} radius={[4, 4, 0, 0]} />
              </>
            )}
          </BarChart>
        </ResponsiveContainer>
      </ChartCard>

      <AnalyticsTable data={data} />
    </div>
  );
}
