import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@fluxeme/shared/src/components/ui/table';
import type { AdminBillingTeamRow } from '@fluxeme/shared/src/types';
import type { BillingCopy } from './types';

type TeamSortKey = 'team_name' | 'total_cost' | 'total_requests' | 'total_tokens' | 'active_users';

type Props = {
  copy: BillingCopy;
  items: AdminBillingTeamRow[];
  selectedTeamId: string | null;
  onSelectTeam: (teamId: string) => void;
  onOpenTeam: (teamId: string) => void;
  fmtCurrency: (amount: number) => string;
  compactNumber: (value: number) => string;
  subtitle: string;
  countLabel: string;
  sortKey: TeamSortKey;
  sortDirection: 'asc' | 'desc';
  onSort: (key: TeamSortKey) => void;
};

export function BillingTeamsTable({
  copy,
  items,
  selectedTeamId,
  onSelectTeam,
  onOpenTeam,
  fmtCurrency,
  compactNumber,
  subtitle,
  countLabel,
  sortKey,
  sortDirection,
  onSort,
}: Props) {
  const headerClass = (key: TeamSortKey) => sortKey === key ? 'text-[#4a5668]' : 'text-[#8792a4]';
  const headerSuffix = (key: TeamSortKey) => (sortKey === key ? (sortDirection === 'asc' ? ' ▲' : ' ▼') : '');

  return (
    <div className="overflow-hidden rounded-[13px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
      <div className="flex items-center justify-between gap-3 border-b border-[#eef2f6] px-[15px] py-[13px]">
        <div>
          <div className="text-[13px] font-[700] text-[#182033]">{copy.teamTableTitle}</div>
          <div className="mt-[3px] text-[9px] text-[#7c8798]">{subtitle}</div>
        </div>
        <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{countLabel}</span>
      </div>
      <div className="max-h-[390px] overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className="border-b-[#e6ebf2] bg-[#fafbfd] hover:bg-[#fafbfd]">
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('team_name')}`} onClick={() => onSort('team_name')}>
                {copy.teamTableTitle}{headerSuffix('team_name')}
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
              <TableHead className={`cursor-pointer px-[11px] py-[9px] text-[9px] font-[650] ${headerClass('active_users')}`} onClick={() => onSort('active_users')}>
                {copy.activeUsers}{headerSuffix('active_users')}
              </TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.avgUnitCost}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.action}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {items.length > 0 ? items.map((team) => {
              const unitCost = team.total_tokens > 0 ? (team.total_cost / team.total_tokens) * 1_000_000 : 0;
              const isSelected = selectedTeamId === team.team_id;
              return (
                <TableRow
                  key={team.team_id}
                  className={isSelected ? 'cursor-pointer bg-[#f2f4ff]' : 'cursor-pointer hover:bg-[#fafbff]'}
                  onClick={() => onSelectTeam(team.team_id)}
                >
                  <TableCell className="px-[11px] py-[11px]">
                    <div className="font-[680] text-[#182033]">{team.team_name}</div>
                    <div className="text-[8px] text-[#9ba4b3]">{team.team_id}</div>
                  </TableCell>
                  <TableCell className="px-[11px] py-[11px] font-mono font-[720] text-[#182033]">{fmtCurrency(team.total_cost)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{compactNumber(team.total_requests)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{compactNumber(team.total_tokens)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{compactNumber(team.active_users)}</TableCell>
                  <TableCell className={`px-[11px] py-[11px] font-mono font-[680] ${unitCost > 1.3 ? 'text-[#d94f5f]' : 'text-[#159467]'}`}>
                    {fmtCurrency(unitCost)}
                  </TableCell>
                  <TableCell className="px-[11px] py-[11px]">
                    <button
                      type="button"
                      onClick={(event) => {
                        event.stopPropagation();
                        onOpenTeam(team.team_id);
                      }}
                      className="text-[10px] font-[650] text-[#5268ff]"
                    >
                      {copy.openTeam}
                    </button>
                  </TableCell>
                </TableRow>
              );
            }) : (
              <TableRow>
                <TableCell colSpan={7} className="py-12 text-center text-sm text-[#7b8496]">{copy.noTeams}</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
