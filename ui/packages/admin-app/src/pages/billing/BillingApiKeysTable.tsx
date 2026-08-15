import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@fluxeme/shared/src/components/ui/table';
import type { AdminBillingUserApiKeyCostRow } from '@fluxeme/shared/src/types';
import type { BillingCopy } from './types';

type Props = {
  copy: BillingCopy;
  items: AdminBillingUserApiKeyCostRow[];
  selectedApiKeyName: string | null;
  onSelectApiKey: (apiKeyName: string | null) => void;
  fmtCurrency: (amount: number) => string;
  compactNumber: (value: number) => string;
  subtitle: string;
  countLabel: string;
  selectedTeamName: string | null;
  selectedUserName: string | null;
  emptyStateLabel: string;
};

export function BillingApiKeysTable({
  copy,
  items,
  selectedApiKeyName,
  onSelectApiKey,
  fmtCurrency,
  compactNumber,
  subtitle,
  countLabel,
  selectedTeamName,
  selectedUserName,
  emptyStateLabel,
}: Props) {
  return (
    <div className="overflow-hidden rounded-[13px] border border-[#e6ebf2] bg-white shadow-[0_8px_24px_rgba(20,30,55,.05)]">
      <div className="flex items-center justify-between gap-3 border-b border-[#eef2f6] px-[15px] py-[13px]">
        <div>
          <div className="text-[13px] font-[700] text-[#182033]">{copy.apiKeyTableTitle}</div>
          <div className="mt-[3px] text-[9px] text-[#7c8798]">{subtitle}</div>
        </div>
        <span className="rounded-full bg-[#f2f4f7] px-[7px] py-[4px] text-[9px] font-[650] text-[#6f7989]">{countLabel}</span>
      </div>
      <div className="max-h-[390px] overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className="border-b-[#e6ebf2] bg-[#fafbfd] hover:bg-[#fafbfd]">
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.apiKeyTableTitle}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.userLabel}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.teamLabel}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.amount}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.totalRequests}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.totalTokens}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.primaryModel}</TableHead>
              <TableHead className="px-[11px] py-[9px] text-[9px] font-[650] text-[#8792a4]">{copy.action}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {items.length > 0 ? items.map((row) => {
              const rowKey = `${row.api_key_name ?? 'unnamed'}-${row.last_request_at ?? ''}`;
              const isSelected = (row.api_key_name ?? null) === selectedApiKeyName;
              return (
                <TableRow key={rowKey} className={isSelected ? 'bg-[#f2f4ff]' : 'hover:bg-[#fafbff]'}>
                  <TableCell className="px-[11px] py-[11px]">
                    <div className="font-[680] text-[#182033]">{row.api_key_name || 'Unnamed Key'}</div>
                    <div className="text-[8px] text-[#9ba4b3]">{row.last_request_at ?? '—'}</div>
                  </TableCell>
                  <TableCell className="px-[11px] py-[11px]">{selectedUserName ?? '—'}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{selectedTeamName ?? '—'}</TableCell>
                  <TableCell className="px-[11px] py-[11px] font-mono font-[720] text-[#182033]">{fmtCurrency(row.total_cost)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{compactNumber(row.total_requests)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{compactNumber(row.total_tokens)}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">{row.primary_model ?? '—'}</TableCell>
                  <TableCell className="px-[11px] py-[11px]">
                    <button type="button" onClick={() => onSelectApiKey(row.api_key_name ?? null)} className="text-[10px] font-[650] text-[#5268ff]">
                      {copy.openApiKey}
                    </button>
                  </TableCell>
                </TableRow>
              );
            }) : (
              <TableRow>
                <TableCell colSpan={8} className="py-12 text-center text-sm text-[#7b8496]">{emptyStateLabel}</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
