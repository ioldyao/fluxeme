import type { BillingCopy } from './types';
import type { AdminBillingUserSpendRow } from '@fluxeme/shared/src/types';
import { BillingKpiRow, type KpiItem } from './BillingKpiRow';
import { BillingUsersTable } from './BillingUsersTable';

type Props = {
  copy: BillingCopy;
  items: AdminBillingUserSpendRow[];
  filteredItems: AdminBillingUserSpendRow[];
  selectedUserId: string | null;
  onSelectUser: (userId: string) => void;
  onOpenUser: (userId: string) => void;
  fmtCurrency: (n: number) => string;
  compactNumber: (n: number) => string;
  subtitle: string;
  countLabel: string;
  sortKey: string;
  sortDirection: 'asc' | 'desc';
  onSort: (key: string) => void;
};

export function UsersView({
  copy,
  items,
  filteredItems,
  selectedUserId,
  onSelectUser,
  onOpenUser,
  fmtCurrency,
  compactNumber,
  subtitle,
  countLabel,
  sortKey,
  sortDirection,
  onSort,
}: Props) {
  const kpis: KpiItem[] = [
    {
      label: '有消费用户',
      value: String(filteredItems.length),
      meta: `${items.length} 个用户中`,
    },
    {
      label: '用户总消费',
      value: fmtCurrency(items.reduce((s, u) => s + u.total_cost, 0)),
      meta: '独立个人账单',
    },
    {
      label: '用户请求',
      value: compactNumber(items.reduce((s, u) => s + u.total_requests, 0)),
      meta: `${compactNumber(items.reduce((s, u) => s + u.total_tokens, 0))} Token`,
    },
    {
      label: '无团队用户',
      value: String(items.filter((u) => !u.team_id).length),
      meta: '个人账单正常存在',
    },
  ];

  return (
    <div className="space-y-[14px]">
      <BillingKpiRow items={kpis} />
      <BillingUsersTable
        copy={copy}
        items={filteredItems}
        selectedUserId={selectedUserId}
        onSelectUser={onSelectUser}
        onOpenUser={onOpenUser}
        fmtCurrency={fmtCurrency}
        compactNumber={compactNumber}
        subtitle={subtitle}
        countLabel={countLabel}
        sortKey={sortKey as any}
        sortDirection={sortDirection}
        onSort={onSort}
      />
    </div>
  );
}
