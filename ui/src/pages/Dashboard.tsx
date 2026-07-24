import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Activity, BarChart3, Braces, CreditCard, Gauge, Key, Layers3, Wallet } from 'lucide-react';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@/components/ui/button';
import { usePermission } from '@/permissions';
import { useCurrency, CURRENCY_SYMBOL } from '@/store/currency';
import { useDashboard, useDashboardAggregations } from '@/api/dashboard';
import { useSubscriptions } from '@/api/models';
import { useUsage, useUsageAggregate, useModelActivity } from '@/api/usage';
import { useEstimatedDays, useWalletOverview } from '@/api/wallet';
import { DashboardToolbar } from '@/components/dashboard/DashboardToolbar';
import { DashboardStatsGrid } from '@/components/dashboard/DashboardStatsGrid';
import { DashboardChartsSection } from '@/components/dashboard/DashboardChartsSection';
import { DashboardRecentUsageCard } from '@/components/dashboard/DashboardRecentUsageCard';
import { DashboardQuickActionsCard } from '@/components/dashboard/DashboardQuickActionsCard';
import { DashboardInfoSection } from '@/components/dashboard/DashboardInfoSection';
import { buildDashboardStats, getDashboardModelShare, getUsageChartData } from '@/components/dashboard/dashboardViewModel';

export default function Dashboard() {
  const { t } = useTranslation();
  const [days, setDays] = useState(14);

  const { data: stats, isLoading, isError, refetch } = useDashboard();
  const { data: agg, isLoading: isAggregationsLoading, isError: isAggregationsError, refetch: refetchAggregations } = useDashboardAggregations();
  const { data: subscriptions, isLoading: isSubscriptionsLoading, isError: isSubscriptionsError, refetch: refetchSubscriptions } = useSubscriptions();
  const { data: usageAggregate, isLoading: isUsageAggregateLoading, isError: isUsageAggregateError, refetch: refetchUsageAggregate } = useUsageAggregate(days);
  const { data: modelActivity, isLoading: isModelActivityLoading, isError: isModelActivityError, refetch: refetchModelActivity } = useModelActivity(days);
  const { data: recentUsage, isLoading: isRecentUsageLoading, isError: isRecentUsageError, refetch: refetchRecentUsage } = useUsage({ limit: 5 });
  const { data: walletOverview, isLoading: isWalletLoading, isError: isWalletError, refetch: refetchWalletOverview } = useWalletOverview();
  const { data: estimatedDays, isLoading: isEstimatedDaysLoading, isError: isEstimatedDaysError, refetch: refetchEstimatedDays } = useEstimatedDays();
  const { currency, rate } = useCurrency();
  const currencySymbol = CURRENCY_SYMBOL[currency];
  const isAdmin = usePermission('admin:dashboard');

  const statItems = buildDashboardStats({
    isAdmin,
    stats,
    aggregations: agg,
    subscriptionsCount: subscriptions?.length ?? 0,
    walletOverview,
    estimatedDays: estimatedDays?.days,
    currencySymbol,
    currencyRate: rate,
    labels: {
      requests: t('usage.requests'),
      requestsWindow: t('dash.last24Hours'),
      cost: t('dash.cost24h'),
      costWindow: t('dash.last24Hours'),
      successRate: t('dash.successRate'),
      successRateWindow: t('dash.last24Hours'),
      avgLatency: t('dash.avgLatency'),
      avgLatencyWindow: t('dash.last24Hours'),
      totalTokens: t('usage.totalTokens'),
      totalTokensWindow: t('dash.last24Hours'),
      apiKeys: t('dash.apiKeys'),
      apiKeysStatus: t('dash.apiKeysStatus'),
      models: t('dash.models'),
      modelsStatus: t('dash.modelsStatus'),
      balance: t('wallet.currentBalance'),
      balanceStatus: t('dash.balanceStatus'),
      estimatedDays: t('wallet.estimatedDays'),
      estimatedDaysStatus: t('dash.balanceStatus'),
      unavailable: t('common.unknown'),
      days: t('common.days'),
    },
    icons: {
      requests: <BarChart3 className="size-5" />,
      cost: <Wallet className="size-5" />,
      successRate: <Layers3 className="size-5" />,
      avgLatency: <Gauge className="size-5" />,
      totalTokens: <Activity className="size-5" />,
      apiKeys: <Key className="size-5" />,
      models: <Braces className="size-5" />,
      balance: <CreditCard className="size-5" />,
    },
    queryStatus: {
      statsError: isError,
      aggregationsError: isAggregationsError,
      subscriptionsError: isSubscriptionsError,
      walletError: isWalletError,
      estimatedDaysError: isEstimatedDaysError,
    },
  });

  const modelShare = useMemo(() => getDashboardModelShare(modelActivity, t('dash.otherModels')), [modelActivity, t]);
  const usageChartData = useMemo(() => getUsageChartData(days, usageAggregate), [days, usageAggregate]);
  const isStatsLoading = isLoading
    || isAggregationsLoading
    || isSubscriptionsLoading
    || isWalletLoading
    || isEstimatedDaysLoading;
  const isUsageChartLoading = isUsageAggregateLoading;
  const isUsageChartError = isUsageAggregateError;
  const isModelChartLoading = isModelActivityLoading;
  const isModelChartError = isModelActivityError;

  const handleRefresh = () => {
    void refetch();
    void refetchAggregations();
    void refetchUsageAggregate();
    void refetchModelActivity();
    void refetchRecentUsage();
    void refetchWalletOverview();
    void refetchEstimatedDays();
    void refetchSubscriptions();
  };

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title={t('dash.title')}
        description={t('dash.subtitle')}
        actions={(
          <Button variant="outline" size="sm" onClick={handleRefresh}>
            {t('common.refresh')}
          </Button>
        )}
      />

      <>
        <DashboardToolbar
          days={days}
          onDaysChange={setDays}
          onRefresh={handleRefresh}
        />

        <DashboardStatsGrid items={statItems} isLoading={isStatsLoading} />

        <DashboardChartsSection
          usageData={usageChartData}
          modelShare={modelShare}
          isUsageLoading={isUsageChartLoading}
          isUsageError={isUsageChartError}
          isModelLoading={isModelChartLoading}
          isModelError={isModelChartError}
        />

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <DashboardRecentUsageCard
            records={recentUsage?.records ?? []}
            isLoading={isRecentUsageLoading}
            isError={isRecentUsageError}
          />
          <DashboardQuickActionsCard />
        </div>

        <DashboardInfoSection />
      </>
    </div>
  );
}
