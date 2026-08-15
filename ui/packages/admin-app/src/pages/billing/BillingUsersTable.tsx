import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@fluxeme/shared/src/components/ui/table';
import type { BillingCopy, BillingUserRow } from './types';

type UserSortKey = 'user_name' | 'team_name' | 'total_cost' | 'total_requests' | 'total_tokens' | 'api_key_count';

type Props = {
  copy: BillingCopy;
  items: BillingUserRow[];
  selectedUserId: string | null;
  onSelectUser: (userId: string) => void;
  onOpenUser: (userId: string) => void;
  fmtCurrency: (amount: number) => string;
  compactNumber: (value: number) => string;
  subtitle: string;
  countLabel: string;
  sortKey: UserSortKey;
  sortDirection: 'asc' | 'desc';
  onSort: (key: UserSortKey) => void;
};

export function BillingUsersTable({
  copy,
  items,
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
  const headerClass = (key: UserSortKey) => sortKey === key ? 'text-[#4a5668]' : 'text-[#8792a4]';
  const headerSuffix = (key: UserSortKey) => (sortKey === key ? (sortDirection === 'asc' ? ' ▲' : ' ▼') : '');

  return (
    <div className="overflow-hidden rounded-[13px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
      <div className="flex items-center justify-between gap-3 border-b border-[#eef2f6] px-[15px] py-[13px]">
        <div>
          <div className="text-[13px] font-[700] text-[#182033]">{copy.userTableTitle}</div>
          <div className="mt-[3px] text-[9px] text-[#7c8798]">{subtitle}</div>
        </div>
        <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{countLabel}</span>
      </div>
      <div className="max-h-[390px] overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className="border-b-[#e6ebf2] bg-[#fafbfd] hover:bg-[#fafbfd]">
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('user_name')}`} onClick={() => onSort('user_name')}>
                {copy.userTableTitle}{headerSuffix('user_name')}
              </TableHead>
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('team_name')}`} onClick={() => onSort('team_name')}>
                {copy.teamLabel}{headerSuffix('team_name')}
              </TableHead>
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('total_cost')}`} onClick={() => onSort('total_cost')}>
                {copy.amount}{headerSuffix('total_cost')}
              </TableHead>
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('total_requests')}`} onClick={() => onSort('total_requests')}>
                {copy.totalRequests}{headerSuffix('total_requests')}
              </TableHead>
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('total_tokens')}`} onClick={() => onSort('total_tokens')}>
                {copy.totalTokens}{headerSuffix('total_tokens')}
              </TableHead>
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('api_key_count')}`} onClick={() => onSort('api_key_count')}>
                API Key{headerSuffix('api_key_count')}
              </TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.latestRequest}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.action}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {items.length > 0 ? items.map((user) => (
              <TableRow
                key={user.user_id}
                className={selectedUserId === user.user_id ? 'cursor-pointer bg-[#f2f4ff]' : 'cursor-pointer hover:bg-[#fafbff]'}
                onClick={() => onSelectUser(user.user_id)}
              >
                <TableCell className="px-[11px] py-[11px]">
                  <div className="font-[680] text-[#182033]">{user.user_name}</div>
                  <div className="text-[8px] text-[#9ba4b3]">{user.user_id}</div>
                </TableCell>
                <TableCell className="px-[11px] py-[11px]">{user.multi_team ? `${user.team_name ?? '—'} +${Math.max((user.team_count ?? 1) - 1, 0)}` : (user.team_name ?? '—')}</TableCell>
                <TableCell className="px-[11px] py-[11px] font-mono font-[720] text-[#182033]">{fmtCurrency(user.total_cost)}</TableCell>
                <TableCell className="px-[11px] py-[11px]">{compactNumber(user.total_requests)}</TableCell>
                <TableCell className="px-[11px] py-[11px]">{compactNumber(user.total_tokens)}</TableCell>
                <TableCell className="px-[11px] py-[11px]">{compactNumber(user.api_key_count ?? 0)}</TableCell>
                <TableCell className="px-[11px] py-[11px] text-[#7b8496]">{user.last_billed_at ?? '—'}</TableCell>
                <TableCell className="px-[11px] py-[11px]">
                  <button
                    type="button"
                    onClick={(event) => {
                      event.stopPropagation();
                      onOpenUser(user.user_id);
                    }}
                    className="text-[10px] font-[650] text-[#5268ff]"
                  >
                    {copy.openUser}
                  </button>
                </TableCell>
              </TableRow>
            )) : (
              <TableRow>
                <TableCell colSpan={8} className="py-12 text-center text-sm text-[#7b8496]">{copy.noUsers}</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
