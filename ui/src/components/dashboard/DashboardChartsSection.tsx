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
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import type { DailyUsage, TopModel } from '@/types';
import { DashboardChartTooltip } from './DashboardChartTooltip';

const CHART_COLORS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-3)', 'var(--chart-4)', 'var(--chart-5)'];

type DashboardChartsSectionProps = {
  dailyData?: DailyUsage[];
  modelShare: TopModel[];
};

export function DashboardChartsSection({ dailyData, modelShare }: DashboardChartsSectionProps) {
  const { t } = useTranslation();

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
      <Card className="card-hover lg:col-span-2">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="size-4 text-brand" />
            {t('dash.requests')}
          </CardTitle>
          <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          {dailyData && dailyData.length > 0 ? (
            <ResponsiveContainer width="100%" height={260}>
              <AreaChart data={dailyData} margin={{ left: -12, right: 8, top: 4 }}>
                <defs>
                  <linearGradient id="reqFill" x1="0" y1="0" x2="0" y2="1">
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
                />
                <Tooltip content={<DashboardChartTooltip />} />
                <Area
                  type="monotone"
                  dataKey="count"
                  name={t('dash.requests')}
                  stroke="var(--chart-1)"
                  strokeWidth={2}
                  fill="url(#reqFill)"
                />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
          )}
        </CardContent>
      </Card>

      <Card className="card-hover">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Braces className="size-4 text-brand" />
            {t('dash.topModels')}
          </CardTitle>
          <CardDescription>{t('dash.usageOverviewSub')}</CardDescription>
        </CardHeader>
        <CardContent>
          {modelShare.length > 0 ? (
            <>
              <ResponsiveContainer width="100%" height={180}>
                <PieChart>
                  <Pie
                    data={modelShare}
                    dataKey="count"
                    nameKey="model"
                    innerRadius={48}
                    outerRadius={72}
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
              <div className="mt-2 space-y-1.5">
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
            <p className="py-8 text-center text-sm text-muted-foreground">{t('dash.noData')}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
