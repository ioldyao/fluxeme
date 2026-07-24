import { useTranslation } from 'react-i18next';
import {
  Area,
  AreaChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { BarChart3, Braces } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader } from '@/components/ui/card';
import type { TopModel } from '@/types';
import { DashboardChartTooltip } from './DashboardChartTooltip';

const CHART_COLORS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'];

type UsageChartPoint = {
  date: string;
  total_tokens: number;
};

type DashboardChartsSectionProps = {
  usageData: UsageChartPoint[];
  modelShare: TopModel[];
  isUsageLoading: boolean;
  isUsageError: boolean;
  isModelLoading: boolean;
  isModelError: boolean;
};

export function DashboardChartsSection({
  usageData,
  modelShare,
  isUsageLoading,
  isUsageError,
  isModelLoading,
  isModelError,
}: DashboardChartsSectionProps) {
  const { t } = useTranslation();

  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
      <Card className="card-hover">
        <CardHeader>
          <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
            <BarChart3 className="size-4 text-brand" />
            {t('dash.tokenTrend')}
          </h2>
          <CardDescription>{t('dash.tokenTrendSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          {isUsageLoading ? (
            <div className="space-y-3">
              {Array.from({ length: 5 }).map((_, index) => (
                <div key={index} className="h-12 animate-pulse rounded-lg bg-muted/60" />
              ))}
            </div>
          ) : isUsageError ? (
            <p className="py-10 text-center text-sm text-destructive">{t('err.loadFailed')}</p>
          ) : usageData.length > 0 ? (
            <ResponsiveContainer width="100%" height={300}>
              <AreaChart data={usageData} margin={{ left: -12, right: 8, top: 4 }}>
                <defs>
                  <linearGradient id="tokenFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.35} />
                    <stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" vertical={false} />
                <XAxis
                  dataKey="date"
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                  tickFormatter={(value: string) => value.slice(5)}
                />
                <YAxis
                  tickLine={false}
                  axisLine={false}
                  tick={{ fill: 'var(--muted-foreground)', fontSize: 12 }}
                  tickFormatter={(value: number) => value.toLocaleString()}
                />
                <Tooltip content={<DashboardChartTooltip />} />
                <Area
                  type="monotone"
                  dataKey="total_tokens"
                  name={t('usage.totalTokens')}
                  stroke="var(--chart-1)"
                  strokeWidth={2}
                  fill="url(#tokenFill)"
                />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <p className="py-10 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
          )}
        </CardContent>
      </Card>

      <Card className="card-hover">
        <CardHeader>
          <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
            <Braces className="size-4 text-brand" />
            {t('dash.modelDistribution')}
          </h2>
          <CardDescription>{t('dash.modelDistributionSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          {isModelLoading ? (
            <div className="space-y-3">
              {Array.from({ length: 4 }).map((_, index) => (
                <div key={index} className="h-12 animate-pulse rounded-lg bg-muted/60" />
              ))}
            </div>
          ) : isModelError ? (
            <p className="py-10 text-center text-sm text-destructive">{t('err.loadFailed')}</p>
          ) : modelShare.length > 0 ? (
            <>
              <ResponsiveContainer width="100%" height={220}>
                <PieChart>
                  <Pie
                    data={modelShare}
                    dataKey="count"
                    nameKey="model"
                    innerRadius={56}
                    outerRadius={84}
                    paddingAngle={2}
                    strokeWidth={0}
                  >
                    {modelShare.map((entry, index) => (
                      <Cell key={entry.model} fill={CHART_COLORS[index % CHART_COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip content={<DashboardChartTooltip />} />
                </PieChart>
              </ResponsiveContainer>
              <div className="sr-only" aria-live="polite">
                <p>{t('dash.modelDistributionSummary')}</p>
                <ul>
                  {modelShare.map((item) => (
                    <li key={item.model}>{item.model}: {item.percentage.toFixed(1)}%</li>
                  ))}
                </ul>
              </div>
              <div className="mt-4 space-y-2">
                {modelShare.map((item, index) => (
                  <div key={item.model} className="flex items-center gap-2 text-sm">
                    <span
                      className="size-2.5 shrink-0 rounded-full"
                      style={{ background: CHART_COLORS[index % CHART_COLORS.length] }}
                    />
                    <span className="flex-1 truncate text-muted-foreground">{item.model}</span>
                    <span className="font-medium">{item.percentage.toFixed(1)}%</span>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <p className="py-10 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
