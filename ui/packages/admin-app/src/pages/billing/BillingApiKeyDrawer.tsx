import { Dialog, DialogContent } from '@fluxeme/shared/src/components/ui/dialog';
import type { AdminBillingApiKeyDetailResponse, UsageRecord } from '@fluxeme/shared/src/types';
import type { BillingCopy } from './types';

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  copy: BillingCopy;
  detail: AdminBillingApiKeyDetailResponse | undefined;
  selectedApiKeyName: string | null;
  selectedRequestId: string | null;
  onSelectRequest: (requestId: string) => void;
  requestDetail: UsageRecord | undefined;
  requestLoading: boolean;
  requestError: Error | null;
  fmtCurrency: (amount: number) => string;
  compactNumber: (value: number) => string;
};

export function BillingApiKeyDrawer({
  open,
  onOpenChange,
  copy,
  detail,
  selectedApiKeyName,
  selectedRequestId,
  onSelectRequest,
  requestDetail,
  requestLoading,
  requestError,
  fmtCurrency,
  compactNumber,
}: Props) {
  const requestCost = requestDetail
    ? ((requestDetail.prompt_price ?? 0) * requestDetail.prompt_tokens
      + (requestDetail.completion_price ?? 0) * requestDetail.completion_tokens
      + (requestDetail.cache_read_price ?? 0) * requestDetail.cache_hit_input_tokens) / 1_000_000
    : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="!left-auto !right-0 !top-0 !h-screen !w-[440px] !max-w-[440px] !translate-x-0 !translate-y-0 gap-0 overflow-y-auto rounded-none border-l border-[#e6ebf2] bg-white p-0 shadow-[-18px_0_45px_rgba(25,34,55,.15)]"
      >
        <div className="flex items-start justify-between border-b border-[#e6ebf2] p-[18px]">
          <div>
            <div className="text-[16px] font-[750] text-[#182033]">{selectedApiKeyName ?? copy.drawerTitle}</div>
            <div className="mt-1 text-[12px] text-[#7c8798]">
              {detail ? `${detail.user_id}${detail.team?.team_name ? ` · ${detail.team.team_name}` : ''}` : copy.drawerHint}
            </div>
          </div>
          <button type="button" onClick={() => onOpenChange(false)} className="grid h-[30px] w-[30px] place-items-center rounded-[8px] bg-[#f1f3f6] text-[#4c586b]">×</button>
        </div>

        <div className="space-y-[18px] p-4">
          <div className="space-y-2">
            <div className="text-[11px] font-[700] text-[#182033]">{copy.currentScopeOverview}</div>
            {detail ? (
              <div className="grid grid-cols-2 gap-[9px]">
                <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                  <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.amount}</small>
                  <b className="text-[13px] text-[#182033]">{requestCost != null ? fmtCurrency(requestCost) : '—'}</b>
                </div>
                <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                  <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.totalRequests}</small>
                  <b className="text-[13px] text-[#182033]">{compactNumber(detail.total_requests)}</b>
                </div>
                <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                  <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.totalTokens}</small>
                  <b className="text-[13px] text-[#182033]">{compactNumber(detail.total_tokens)}</b>
                </div>
                <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                  <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.primaryModel}</small>
                  <b className="text-[11px] text-[#182033]">{detail.top_models[0]?.model ?? '—'}</b>
                </div>
              </div>
            ) : <div className="py-8 text-center text-sm text-[#7b8496]">Loading…</div>}
          </div>

          <div className="space-y-2">
            <h3 className="text-[11px] font-[700] text-[#182033]">{copy.topModels}</h3>
            {detail?.top_models.length ? detail.top_models.slice(0, 3).map((row) => (
              <div key={row.model} className="rounded-[9px] border border-[#e6ebf2] p-[10px]">
                <div className="flex justify-between text-[10px]">
                  <b className="text-[#182033]">{row.model}</b>
                  <span className="font-mono text-[#182033]">{compactNumber(row.total_tokens)} {copy.tokens}</span>
                </div>
                <div className="mt-1 text-[8px] text-[#8d97a7]">{compactNumber(row.total_requests)} {copy.requestUnit}</div>
              </div>
            )) : <div className="text-sm text-[#7b8496]">{copy.noData}</div>}
          </div>

          <div className="space-y-2">
            <h3 className="text-[11px] font-[700] text-[#182033]">{copy.recentRequests}</h3>
            {detail?.recent_requests.length ? detail.recent_requests.map((record) => {
              const isSelected = record.request_id === selectedRequestId;
              const rowCost = ((record.prompt_price ?? 0) * record.prompt_tokens + (record.completion_price ?? 0) * record.completion_tokens + (record.cache_read_price ?? 0) * record.cache_hit_input_tokens) / 1_000_000;
              return (
                <button
                  key={record.request_id}
                  type="button"
                  onClick={() => onSelectRequest(record.request_id)}
                  className={`block w-full rounded-[9px] border px-[10px] py-[10px] text-left ${isSelected ? 'border-[#cfd8ff] bg-[#f2f4ff]' : 'border-[#e6ebf2] hover:bg-[#fafbff]'}`}
                >
                  <div className="flex justify-between text-[9px]">
                    <b className="text-[#182033]">{record.request_id}</b>
                    <span className="font-mono text-[#182033]">{fmtCurrency(rowCost)}</span>
                  </div>
                  <div className="mt-1 text-[8px] text-[#8d97a7]">{compactNumber(record.total_tokens)} {copy.tokens} · {record.model} · {record.timestamp}</div>
                </button>
              );
            }) : <div className="text-sm text-[#7b8496]">{copy.noRequests}</div>}
          </div>

          <div className="space-y-2">
            <h3 className="text-[11px] font-[700] text-[#182033]">{copy.requestDetailTitle}</h3>
            {requestLoading ? (
              <div className="py-8 text-center text-sm text-[#7b8496]">Loading…</div>
            ) : requestError ? (
              <div className="rounded-[9px] border border-dashed border-[#f3d0d4] bg-[#fff7f8] px-4 py-6 text-center text-sm text-[#b94b58]">{requestError.message}</div>
            ) : requestDetail ? (
              <div className="space-y-3">
                <div className="grid grid-cols-2 gap-[9px]">
                  <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                    <small className="mb-[5px] block text-[8px] text-[#7c8798]">Model</small>
                    <b className="text-[11px] text-[#182033]">{requestDetail.original_model ? `${requestDetail.original_model} → ${requestDetail.model}` : requestDetail.model}</b>
                  </div>
                  <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                    <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.teamLabel}</small>
                    <b className="text-[11px] text-[#182033]">{requestDetail.channel_id}</b>
                  </div>
                  <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                    <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.promptLabel}</small>
                    <b className="text-[13px] text-[#182033]">{compactNumber(requestDetail.prompt_tokens)}</b>
                  </div>
                  <div className="rounded-[10px] border border-[#e6ebf2] p-[10px]">
                    <small className="mb-[5px] block text-[8px] text-[#7c8798]">{copy.completionLabel}</small>
                    <b className="text-[13px] text-[#182033]">{compactNumber(requestDetail.completion_tokens)}</b>
                  </div>
                </div>

                <div>
                  <div className="mb-2 text-[10px] font-[650] uppercase tracking-wide text-[#7b8496]">{copy.requestLabel}</div>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-[9px] bg-[#f5f7fb] p-3 text-xs text-[#182033]">{requestDetail.request_body || '(empty)'}</pre>
                </div>

                {requestDetail.reasoning_body ? (
                  <div>
                    <div className="mb-2 text-[10px] font-[650] uppercase tracking-wide text-[#7b8496]">{copy.reasoningLabel}</div>
                    <pre className="max-h-32 overflow-auto whitespace-pre-wrap break-all rounded-[9px] bg-[#f5f7fb] p-3 text-xs text-[#182033]">{requestDetail.reasoning_body}</pre>
                  </div>
                ) : null}

                <div>
                  <div className="mb-2 text-[10px] font-[650] uppercase tracking-wide text-[#7b8496]">{copy.responseLabel}</div>
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-[9px] bg-[#f5f7fb] p-3 text-xs text-[#182033]">{requestDetail.response_body || '(empty)'}</pre>
                </div>
              </div>
            ) : (
              <div className="rounded-[9px] border border-dashed border-[#e6ebf2] bg-[#fbfcfe] px-4 py-8 text-center text-sm text-[#7b8496]">{copy.viewRequest}</div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
