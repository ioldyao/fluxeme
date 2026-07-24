import type { ReactNode } from 'react';
import type { DashboardAggregations } from '@/types';

type DashboardStatItem = {
  title: string;
  value: number;
  icon: ReactNode;
};

type BuildDashboardStatsParams = {
  isAdmin: boolean;
  stats: {
    users?: number;
    channels?: number;
    models?: number;
    api_keys?: number;
    total_requests?: number;
  } | null | undefined;
  subscriptionsCount: number;
  titles: {
    users: string;
    channels: string;
    models: string;
    apiKeys: string;
    requests: string;
  };
  icons: {
    users: ReactNode;
    channels: ReactNode;
    models: ReactNode;
    apiKeys: ReactNode;
    requests: ReactNode;
  };
};

export function buildDashboardStats({
  isAdmin,
  stats,
  subscriptionsCount,
  titles,
  icons,
}: BuildDashboardStatsParams): DashboardStatItem[] {
  if (isAdmin) {
    return [
      { title: titles.users, value: stats?.users ?? 0, icon: icons.users },
      { title: titles.channels, value: stats?.channels ?? 0, icon: icons.channels },
      { title: titles.models, value: stats?.models ?? 0, icon: icons.models },
      { title: titles.apiKeys, value: stats?.api_keys ?? 0, icon: icons.apiKeys },
      { title: titles.requests, value: stats?.total_requests ?? 0, icon: icons.requests },
    ];
  }

  return [
    { title: titles.models, value: subscriptionsCount, icon: icons.models },
    { title: titles.apiKeys, value: stats?.api_keys ?? 0, icon: icons.apiKeys },
    { title: titles.requests, value: stats?.total_requests ?? 0, icon: icons.requests },
  ];
}

export function getDashboardModelShare(aggregations: DashboardAggregations | undefined) {
  return aggregations?.top_models_24h?.slice(0, 5) ?? [];
}

export type { DashboardStatItem };
