import { useTranslation } from 'react-i18next';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@/components/ui/button';
import { usePermission } from '@/permissions';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE } from '@/store/currency';
import { useDashboard, useDashboardAggregations, useDailyUsage } from '@/api/dashboard';
import { useSubscriptions } from '@/api/models';
import { Users, Radio, Braces, Key, Activity } from 'lucide-react';
import { DashboardStatsGrid } from '@/components/dashboard/DashboardStatsGrid';
import { DashboardChartsSection } from '@/components/dashboard/DashboardChartsSection';
import { DashboardOverviewSection } from '@/components/dashboard/DashboardOverviewSection';
import { DashboardInfoSection } from '@/components/dashboard/DashboardInfoSection';
import { buildDashboardStats, getDashboardModelShare } from '@/components/dashboard/dashboardViewModel';

export default function Dashboard() {
  const { t } = useTranslation();
  const { data: stats, isLoading, isError, refetch } = useDashboard();
  const { data: agg } = useDashboardAggregations();
  const { data: dailyData } = useDailyUsage(14);
  const { data: subscriptions } = useSubscriptions();
  const { currency } = useCurrency();
  const currencySymbol = CURRENCY_SYMBOL[currency];
  const currencyCode = CURRENCY_CODE[currency];
  const isAdmin = usePermission('admin:dashboard');

  const statItems = buildDashboardStats({
    isAdmin,
    stats,
    subscriptionsCount: subscriptions?.length ?? 0,
    titles: {
      users: t('dash.users'),
      channels: t('dash.channels'),
      models: t('dash.models'),
      apiKeys: t('dash.apiKeys'),
      requests: t('dash.requests'),
    },
    icons: {
      users: <Users className="size-5" />,
      channels: <Radio className="size-5" />,
      models: <Braces className="size-5" />,
      apiKeys: <Key className="size-5" />,
      requests: <Activity className="size-5" />,
    },
  });

  const modelShare = getDashboardModelShare(agg);

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader title={t('dash.title')} description={t('dash.subtitle')} />

      {isError ? (
        <div className="flex items-center justify-center p-8">
          <div className="text-center">
            <p className="mb-2 text-destructive">{t('err.loadFailed')}</p>
            <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
          </div>
        </div>
      ) : (
        <>
          <DashboardStatsGrid items={statItems} isLoading={isLoading} />

          {agg && (
            <>
              <DashboardChartsSection dailyData={dailyData} modelShare={modelShare} />
              <DashboardOverviewSection
                aggregations={agg}
                currencySymbol={currencySymbol}
                currencyCode={currencyCode}
                modelShare={modelShare}
              />
            </>
          )}

          <DashboardInfoSection />
        </>
      )}
    </div>
  );
}
