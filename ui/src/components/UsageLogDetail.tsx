import { useTranslation } from 'react-i18next';
import { useUsageDetail } from '@/api/usage';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Skeleton } from '@/components/ui/skeleton';

interface Props {
  requestId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function UsageLogDetail({ requestId, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const { data: record, isLoading, error } = useUsageDetail(requestId);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="text-xl">{t('usage.detailTitle')}</DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="space-y-3 p-4">
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="h-20 w-full" />
          </div>
        ) : record ? (
          <div className="space-y-5 min-w-0">

            {/* Meta info row */}
            <div className="grid grid-cols-2 lg:grid-cols-6 gap-3">
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{t('table.user')}</div>
                <div className="text-sm font-medium truncate">{record.user_name}</div>
              </div>
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{t('table.model')}</div>
                {record.original_model ? (
                  <div className="space-y-0.5">
                    <div className="text-sm font-medium truncate">{record.model}</div>
                    <div className="text-xs text-muted-foreground/60 truncate">← {record.original_model}</div>
                  </div>
                ) : (
                  <div className="text-sm font-medium truncate">{record.model}</div>
                )}
              </div>
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{t('usage.apiKey')}</div>
                <div className="text-sm font-medium truncate">{record.api_key_name ?? '—'}</div>
              </div>
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{t('usage.apiFormat')}</div>
                <div className="text-sm font-medium truncate">{record.api_format ?? '—'}</div>
              </div>
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{t('usage.channel')}</div>
                <div className="text-sm font-medium truncate">{record.channel_id}</div>
              </div>
              <div className="rounded-lg border bg-card p-3">
                <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">Client IP</div>
                <div className="text-sm font-medium truncate">{record.client_ip ?? '—'}</div>
              </div>
            </div>

            {/* Token & Cost row */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
              {[
                { label: t('usage.promptTokens'), value: record.prompt_tokens.toLocaleString() },
                { label: t('usage.cacheHit'), value: record.cache_hit_input_tokens > 0 ? record.cache_hit_input_tokens.toLocaleString() : '—' },
                { label: t('usage.completionTokens'), value: record.completion_tokens.toLocaleString() },
                { label: t('usage.totalTokens'), value: record.total_tokens.toLocaleString() },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-medium truncate">{m.value}</div>
                </div>
              ))}
            </div>

            {/* Status row */}
            <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
              {[
                { label: t('usage.latency'), value: `${record.latency_ms}ms` },
                { label: t('usage.status'), value: record.success ? '✅ Success' : `❌ ${record.status_code}` },
                { label: t('usage.apiFormat'), value: record.stream ? 'Streaming' : 'Non-streaming' },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-medium truncate">{m.value}</div>
                </div>
              ))}
            </div>

            {/* Request body */}
            {record.request_body && (
              <div className="space-y-2">
                <h4 className="text-sm font-semibold">{t('usage.detailTitle')}</h4>
                <div className="max-h-48 overflow-y-auto">
                  <pre className="text-xs leading-relaxed whitespace-pre-wrap break-all font-mono bg-muted rounded-lg p-3 border">
                    {formatJson(record.request_body)}
                  </pre>
                </div>
              </div>
            )}
          </div>
        ) : error ? (
          <div className="p-8 text-center text-destructive">{t('err.loadFailed')}</div>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function formatJson(s: string): string {
  try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return s; }
}
