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
import { useChannels } from '@/api/channels';
import { useRoutingHistory } from '@/api/routing';
import { DashboardToolbar } from '@/components/dashboard/DashboardToolbar';
import { DashboardStatsGrid } from '@/components/dashboard/DashboardStatsGrid';
import { DashboardChartsSection } from '@/components/dashboard/DashboardChartsSection';
import { DashboardRecentUsageCard } from '@/components/dashboard/DashboardRecentUsageCard';
import { DashboardQuickActionsCard } from '@/components/dashboard/DashboardQuickActionsCard';
import { DashboardInfoSection } from '@/components/dashboard/DashboardInfoSection';
import { DashboardAdminSection } from '@/components/dashboard/admin/DashboardAdminSection';
import { buildDashboardStats, getDashboardModelShare, getUsageChartData } from '@/components/dashboard/dashboardViewModel';

export default function Dashboard() {
  const { t } = useTranslation();
  const [days, setDays] = useState(14);

  const isAdmin = usePermission('admin:dashboard');
  const { data: stats, isLoading, isError, refetch } = useDashboard();
  const { data: agg, isLoading: isAggregationsLoading, isError: isAggregationsError, refetch: refetchAggregations } = useDashboardAggregations();
  const { data: subscriptions, isLoading: isSubscriptionsLoading, isError: isSubscriptionsError, refetch: refetchSubscriptions } = useSubscriptions();
  const { data: usageAggregate, isLoading: isUsageAggregateLoading, isError: isUsageAggregateError, refetch: refetchUsageAggregate } = useUsageAggregate(days);
  const { data: modelActivity, isLoading: isModelActivityLoading, isError: isModelActivityError, refetch: refetchModelActivity } = useModelActivity(days);
  const { data: recentUsage, isLoading: isRecentUsageLoading, isError: isRecentUsageError, refetch: refetchRecentUsage } = useUsage({ limit: 8 });
  const { data: walletOverview, isLoading: isWalletLoading, isError: isWalletError, refetch: refetchWalletOverview } = useWalletOverview();
  const { data: estimatedDays, isLoading: isEstimatedDaysLoading, isError: isEstimatedDaysError, refetch: refetchEstimatedDays } = useEstimatedDays();
  const { data: channels } = useChannels(undefined, { enabled: isAdmin });
  const { currency, rate } = useCurrency();
  const currencySymbol = CURRENCY_SYMBOL[currency];
  const {
    data: routingHistory,
    isLoading: isRoutingLoading,
    isError: isRoutingError,
    refetch: refetchRoutingHistory,
  } = useRoutingHistory(days, { enabled: isAdmin });

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
  const routingRows = useMemo(() => {
    if (!routingHistory) {
      return [];
    }

    const total = routingHistory.summary.reduce((sum, row) => sum + row.requests, 0);

    return routingHistory.summary
      .slice()
      .sort((a, b) => b.requests - a.requests)
      .slice(0, 3)
      .map((row, index) => ({
        channelId: row.channel_id,
        channelName: routingHistory.series[row.channel_id]?.channel_name ?? row.channel_id,
        routeRole: index === 0 ? t('dash.routeTrafficTop') : index === 1 ? t('dash.routeTrafficSecond') : t('dash.routeTrafficThird'),
        share: total > 0 ? (row.requests / total) * 100 : 0,
        requests: row.requests,
        avgLatency: row.avg_latency,
      }));
  }, [routingHistory, t]);
  const riskAlerts = useMemo(() => {
    const alerts: { id: string; title: string; description: string; severity: 'warn' | 'info' }[] = [];

    if ((agg?.avg_latency_ms_24h ?? 0) > 2000) {
      alerts.push({
        id: 'latency',
        title: t('dash.alertLatencyTitle'),
        description: t('dash.alertLatencyDesc', { latency: (agg?.avg_latency_ms_24h ?? 0).toFixed(0) }),
        severity: 'warn',
      });
    }
    if ((agg?.success_rate_24h ?? 100) < 95) {
      alerts.push({
        id: 'success',
        title: t('dash.alertSuccessTitle'),
        description: t('dash.alertSuccessDesc', { rate: (agg?.success_rate_24h ?? 0).toFixed(1) }),
        severity: 'warn',
      });
    }
    if ((modelShare[0]?.percentage ?? 0) > 80) {
      alerts.push({
        id: 'concentration',
        title: t('dash.alertConcentrationTitle'),
        description: t('dash.alertConcentrationDesc', { model: modelShare[0]?.model ?? '—', share: (modelShare[0]?.percentage ?? 0).toFixed(1) }),
        severity: 'info',
      });
    }
    if ((estimatedDays?.days ?? Infinity) < 10) {
      alerts.push({
        id: 'balance',
        title: t('dash.alertBalanceTitle'),
        description: t('dash.alertBalanceDesc', { days: (estimatedDays?.days ?? 0).toFixed(1) }),
        severity: 'warn',
      });
    }

    return alerts;
  }, [agg, estimatedDays?.days, modelShare, t]);
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
    if (isAdmin) {
      void refetchRoutingHistory();
    }
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
            records={(recentUsage?.records ?? []).slice(0, 5)}
            isLoading={isRecentUsageLoading}
            isError={isRecentUsageError}
          />
          <DashboardQuickActionsCard />
        </div>

        <DashboardInfoSection />

        {isAdmin && (
          <DashboardAdminSection
            availability={agg?.success_rate_24h ?? 0}
            modelCount={stats?.models ?? 0}
            apiKeyCount={stats?.api_keys ?? 0}
            channelCount={channels?.length ?? 0}
            requests24h={agg?.requests_24h ?? 0}
            totalTokens24h={agg?.total_tokens_24h ?? 0}
            avgLatencyMs24h={agg?.avg_latency_ms_24h ?? 0}
            cost24hLabel={`${currencySymbol}${((agg?.cost_24h ?? 0) * (currency === 'cny' ? rate : 1)).toFixed(2)}`}
            routingRows={routingRows}
            isRoutingLoading={isRoutingLoading}
            isRoutingError={isRoutingError}
            alerts={riskAlerts}
            requestLogs={recentUsage?.records ?? []}
            isLogsLoading={isRecentUsageLoading}
            isLogsError={isRecentUsageError}
          />
        )}
      </>
    </div>
  );
}
