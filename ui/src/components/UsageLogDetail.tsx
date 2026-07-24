import { useTranslation } from 'react-i18next';
import { useUsageDetail } from '@/api/usage';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { useCurrency } from '@/store/currency';
import { formatCost, getRecordPricing } from '@/lib/cost';
import type { UsageRecord } from '@/types';

interface Props {
  requestId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const COLORS: Record<string, string> = { ok: '#14966a', pending: '#87939e', streaming: '#2d7fb8', warn: '#c97800', fail: '#d84b4b' };

function estimateEvents(record: UsageRecord) {
  const ts = new Date(record.timestamp);
  const tsStr = ts.toLocaleTimeString();
  const events: { cls: string; title: string; time: string; detail: string }[] = [];

  events.push({ cls: 'ok', title: 'Gateway Accepted', time: tsStr, detail: '请求已进入网关' });

  if (record.latency_ms > 0) {
    const authTime = new Date(ts.getTime() + 50);
    events.push({ cls: 'ok', title: 'Auth & Route', time: authTime.toLocaleTimeString(), detail: `${record.api_format ?? 'openai'} · ${record.channel_id}` });

    if (record.success) {
      const startTs = new Date(ts.getTime() + record.latency_ms * 0.3);
      events.push({ cls: record.stream ? 'streaming' : 'ok', title: record.stream ? 'Streaming Started' : 'Provider Processing', time: startTs.toLocaleTimeString(), detail: record.stream ? `Streaming ${record.completion_tokens} tokens` : `Processing ${record.total_tokens} tokens` });
      const endTs = new Date(ts.getTime() + record.latency_ms);
      events.push({ cls: 'ok', title: record.stream ? 'Completed' : 'Response Received', time: endTs.toLocaleTimeString(), detail: `Status ${record.status_code} · ${record.latency_ms}ms` });
    } else {
      const failTs = new Date(ts.getTime() + record.latency_ms);
      events.push({ cls: 'fail', title: 'Failed', time: failTs.toLocaleTimeString(), detail: `Status ${record.status_code} · ${record.latency_ms}ms` });
    }
  }

  return events;
}

function formatJson(val: string | null | undefined) {
  if (!val) return '(empty)';
  try { return JSON.stringify(JSON.parse(val), null, 2); } catch { return val; }
}

function formatResponse(val: string | null | undefined) {
  if (!val) return '(empty)';
  if (!val.trim().startsWith('data:')) return val;
  const lines = val.split('\n').filter(l => l.trim());
  const parsed: string[] = [];
  for (const line of lines) {
    const sse = line.replace(/^data:\s*/, '');
    if (sse === '[DONE]') continue;
    try {
      const d = JSON.parse(sse);
      const content = d.choices?.[0]?.delta?.content || d.choices?.[0]?.delta?.reasoning_content || d.choices?.[0]?.text || '';
      if (content) parsed.push(content);
    } catch { continue; }
  }
  return parsed.length > 0 ? parsed.join('') : val;
}

export function UsageLogDetail({ requestId, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const { data: record, isLoading, error } = useUsageDetail(requestId);
  const { currency, rate } = useCurrency();

  const costStr = record ? formatCost(record.prompt_tokens, record.completion_tokens, record.cache_hit_input_tokens, getRecordPricing(record, {}), currency, rate) : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[90vw] max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t('usage.detailTitle')}
            {record && <span className="font-mono text-xs text-muted-foreground">{record.request_id.substring(0, 12)}</span>}
          </DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : record ? (
          <div className="space-y-5 min-w-0">

            {/* Meta info row */}
            <div className="grid grid-cols-2 lg:grid-cols-6 gap-3">
              {[
                { label: t('table.user'), value: record.user_name },
                { label: t('table.model'), value: record.model },
                { label: t('usage.apiKey'), value: record.api_key_name ?? '—' },
                { label: t('usage.apiFormat'), value: record.api_format ?? '—' },
                { label: t('usage.channel'), value: record.channel_id },
                { label: 'Client IP', value: record.client_ip ?? '—' },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-medium truncate">{m.value}</div>
                </div>
              ))}
            </div>

            {/* Token & Cost row */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
              {[
                { label: t('usage.promptTokens'), value: record.prompt_tokens.toLocaleString() },
                { label: t('usage.cacheHit'), value: record.cache_hit_input_tokens > 0 ? record.cache_hit_input_tokens.toLocaleString() : '—' },
                { label: t('usage.completionTokens'), value: record.completion_tokens.toLocaleString() },
                { label: t('usage.cost'), value: costStr || '—' },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-semibold font-mono">{m.value}</div>
                </div>
              ))}
            </div>

            <hr className="border-border" />

            {/* Request Lifecycle Timeline (Scheme C) */}
            <div>
              <h3 className="text-sm font-semibold mb-3 flex items-center gap-2">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
                {t('usage.requestLifecycle')}
              </h3>
              <div className="grid grid-cols-1 xl:grid-cols-[1.2fr_0.65fr] gap-4">
                {/* Timeline */}
                <div className="relative pl-[34px]">
                  <div className="absolute left-[10px] top-[8px] bottom-[8px] w-[2px] bg-border" />
                  {estimateEvents(record).map((ev, i) => (
                    <div key={i} className="relative pb-4 last:pb-0">
                      <div className="absolute left-[-29px] top-[3px] w-[12px] h-[12px] rounded-full bg-card border-[3px]" style={{ borderColor: COLORS[ev.cls] || COLORS.ok }} />
                      <div className="flex justify-between gap-3">
                        <div className="font-semibold text-sm">{ev.title}</div>
                        <div className="text-[11px] font-mono text-muted-foreground shrink-0">{ev.time}</div>
                      </div>
                      <div className="mt-1 text-xs text-muted-foreground">{ev.detail}</div>
                    </div>
                  ))}
                </div>

                {/* Inspector panel */}
                <div className="rounded-lg border bg-card p-4">
                  <h4 className="text-sm font-semibold mb-2">{t('usage.detailTitle')}</h4>
                  <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium ${record.success ? 'bg-emerald-500/10 text-emerald-700' : 'bg-red-500/10 text-red-700'}`}>
                    <span className={`size-1.5 rounded-full ${record.success ? 'bg-emerald-500' : 'bg-red-500'}`} />
                    {record.success ? t('usage.success') : t('usage.failure')} · HTTP {record.status_code}
                  </span>
                  <div className="mt-3 space-y-0">
                    {[
                      [t('table.status'), `${record.success ? t('usage.success') : t('usage.failure')}`],
                      ['Request ID', record.request_id],
                      [t('table.latency'), `${record.latency_ms}ms`],
                      [t('usage.totalTokens'), record.total_tokens.toLocaleString()],
                      [t('usage.cost'), costStr || '—'],
                    ].map((r, i) => (
                      <div key={i} className="flex justify-between gap-3 py-2 border-t border-border/60 first:border-0">
                        <span className="text-xs text-muted-foreground">{r[0]}</span>
                        <b className="text-xs text-right">{r[1]}</b>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>

            <hr className="border-border" />

            {/* Request body */}
            <div>
              <h4 className="text-sm font-medium mb-2">{t('usage.request')}</h4>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-60 overflow-y-auto whitespace-pre-wrap break-all max-w-full">{formatJson(record.request_body)}</pre>
            </div>

            {/* Reasoning / Thinking */}
            {record.reasoning_body && (
              <div>
                <details>
                  <summary className="text-sm font-medium cursor-pointer select-none">{t('usage.thinking')}</summary>
                  <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-80 overflow-y-auto whitespace-pre-wrap break-all max-w-full mt-2">{record.reasoning_body}</pre>
                </details>
              </div>
            )}

            {/* Response body */}
            <div>
              <h4 className="text-sm font-medium mb-2">{t('usage.output')}</h4>
              <div className="text-xs text-muted-foreground mb-2">{t('usage.reply')}</div>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-80 overflow-y-auto whitespace-pre-wrap break-all max-w-full">{formatResponse(record.response_body)}</pre>
            </div>

          </div>
        ) : error ? (
          <div className="p-8 text-center text-destructive">{error.message}</div>
        ) : (
          <div className="p-8 text-center text-muted-foreground">{t('common.notFound')}</div>
        )}
      </DialogContent>
    </Dialog>
  );
}
