import type { ReactNode } from 'react';
import type { DashboardAggregations, ModelActivity, TopModel } from '@/types';
import type { WalletOverview } from '@/api/wallet';

type DashboardStatItem = {
  title: string;
  value: string;
  subtitle?: string;
  icon: ReactNode;
};

type DashboardQueryStatus = {
  statsError: boolean;
  aggregationsError: boolean;
  subscriptionsError: boolean;
  walletError: boolean;
  estimatedDaysError: boolean;
};

type BuildDashboardStatsParams = {
  isAdmin: boolean;
  stats: {
    models?: number;
    api_keys?: number;
  } | null | undefined;
  aggregations: DashboardAggregations | undefined;
  subscriptionsCount: number;
  walletOverview?: WalletOverview;
  estimatedDays: number | null | undefined;
  currencySymbol: string;
  currencyRate: number;
  queryStatus: DashboardQueryStatus;
  labels: {
    requests: string;
    requestsWindow: string;
    cost: string;
    costWindow: string;
    successRate: string;
    successRateWindow: string;
    avgLatency: string;
    avgLatencyWindow: string;
    totalTokens: string;
    totalTokensWindow: string;
    apiKeys: string;
    apiKeysStatus: string;
    models: string;
    modelsStatus: string;
    balance: string;
    balanceStatus: string;
    estimatedDays: string;
    estimatedDaysStatus: string;
    unavailable: string;
    days: string;
  };
  icons: {
    requests: ReactNode;
    cost: ReactNode;
    successRate: ReactNode;
    avgLatency: ReactNode;
    totalTokens: ReactNode;
    apiKeys: ReactNode;
    models: ReactNode;
    balance: ReactNode;
  };
};

function formatCurrency(symbol: string, rate: number, value: number | undefined) {
  const amount = value ?? 0;
  const converted = symbol === '¥' ? amount * rate : amount;
  return `${symbol}${converted.toFixed(2)}`;
}

function formatLatency(value: number | undefined) {
  const latency = value ?? 0;
  return latency >= 1000 ? `${(latency / 1000).toFixed(2)}s` : `${latency.toFixed(0)}ms`;
}

export function buildDashboardStats({
  isAdmin,
  stats,
  aggregations,
  subscriptionsCount,
  walletOverview,
  estimatedDays,
  currencySymbol,
  currencyRate,
  queryStatus,
  labels,
  icons,
}: BuildDashboardStatsParams): DashboardStatItem[] {
  const modelCount = isAdmin ? (stats?.models ?? 0) : subscriptionsCount;
  const metricsUnavailable = queryStatus.aggregationsError;
  const statsUnavailable = queryStatus.statsError;
  const subscriptionsUnavailable = !isAdmin && queryStatus.subscriptionsError;
  const walletUnavailable = queryStatus.walletError;
  const estimatedDaysUnavailable = queryStatus.estimatedDaysError;

  const unavailableMetric = (title: string, subtitle: string, icon: ReactNode): DashboardStatItem => ({
    title,
    value: '—',
    subtitle,
    icon,
  });

  return [
    metricsUnavailable
      ? unavailableMetric(labels.requests, labels.unavailable, icons.requests)
      : {
          title: labels.requests,
          value: (aggregations?.requests_24h ?? 0).toLocaleString(),
          subtitle: labels.requestsWindow,
          icon: icons.requests,
        },
    metricsUnavailable
      ? unavailableMetric(labels.cost, labels.unavailable, icons.cost)
      : {
          title: labels.cost,
          value: formatCurrency(currencySymbol, currencyRate, aggregations?.cost_24h),
          subtitle: labels.costWindow,
          icon: icons.cost,
        },
    metricsUnavailable
      ? unavailableMetric(labels.successRate, labels.unavailable, icons.successRate)
      : {
          title: labels.successRate,
          value: `${(aggregations?.success_rate_24h ?? 0).toFixed(1)}%`,
          subtitle: labels.successRateWindow,
          icon: icons.successRate,
        },
    metricsUnavailable
      ? unavailableMetric(labels.avgLatency, labels.unavailable, icons.avgLatency)
      : {
          title: labels.avgLatency,
          value: formatLatency(aggregations?.avg_latency_ms_24h),
          subtitle: labels.avgLatencyWindow,
          icon: icons.avgLatency,
        },
    metricsUnavailable
      ? unavailableMetric(labels.totalTokens, labels.unavailable, icons.totalTokens)
      : {
          title: labels.totalTokens,
          value: (aggregations?.total_tokens_24h ?? 0).toLocaleString(),
          subtitle: labels.totalTokensWindow,
          icon: icons.totalTokens,
        },
    statsUnavailable
      ? unavailableMetric(labels.apiKeys, labels.unavailable, icons.apiKeys)
      : {
          title: labels.apiKeys,
          value: (stats?.api_keys ?? 0).toLocaleString(),
          subtitle: labels.apiKeysStatus,
          icon: icons.apiKeys,
        },
    (isAdmin ? statsUnavailable : subscriptionsUnavailable)
      ? unavailableMetric(labels.models, labels.unavailable, icons.models)
      : {
          title: labels.models,
          value: modelCount.toLocaleString(),
          subtitle: labels.modelsStatus,
          icon: icons.models,
        },
    walletUnavailable
      ? unavailableMetric(labels.balance, labels.unavailable, icons.balance)
      : {
          title: labels.balance,
          value: formatCurrency(currencySymbol, currencyRate, walletOverview?.balance),
          subtitle: estimatedDays != null && !estimatedDaysUnavailable
            ? `${labels.estimatedDays}: ${estimatedDays.toFixed(1)} ${labels.days}`
            : labels.balanceStatus,
          icon: icons.balance,
        },
  ];
}

export function getDashboardModelShare(modelActivity: ModelActivity[] | undefined, otherLabel: string): TopModel[] {
  if (!modelActivity?.length) {
    return [];
  }

  const sortedModels = modelActivity
    .slice()
    .sort((a, b) => b.total_requests - a.total_requests);
  const topModels = sortedModels.slice(0, 5);
  const topModelTotal = topModels.reduce((sum, item) => sum + item.total_requests, 0);
  const totalRequests = sortedModels.reduce((sum, item) => sum + item.total_requests, 0);
  const remainingRequests = totalRequests - topModelTotal;

  const share = topModels.map((item) => ({
    model: item.model,
    count: item.total_requests,
    percentage: totalRequests > 0 ? (item.total_requests / totalRequests) * 100 : 0,
  }));

  if (remainingRequests > 0) {
    share.push({
      model: otherLabel,
      count: remainingRequests,
      percentage: totalRequests > 0 ? (remainingRequests / totalRequests) * 100 : 0,
    });
  }

  return share;
}

export function getUsageChartData(_days: number, aggregates: { date: string; total_tokens: number }[] | undefined) {
  if (!aggregates?.length) {
    return [];
  }

  return aggregates.map((entry) => ({
    date: entry.date,
    total_tokens: entry.total_tokens,
  }));
}

export type { DashboardStatItem };
