import { useTranslation } from 'react-i18next';
import { useUsageDetail } from '@/api/usage';
import { useModels } from '@/api/models';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { CheckCircle2, XCircle, Radio, RadioIcon } from 'lucide-react';
import type { Model, Pricing } from '@/types';

interface Props {
  requestId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function UsageLogDetail({ requestId, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const { data: record, isLoading, error } = useUsageDetail(requestId);
  const { data: models } = useModels();

  const isStreaming = (record: { request_body?: string | null }) => {
    if (!record.request_body) return false;
    try {
      const body = JSON.parse(record.request_body);
      return body.stream === true;
    } catch {
      return false;
    }
  };

  const findModel = (modelName: string): Model | undefined => {
    return models?.find(m => m.name === modelName || modelName.startsWith(m.name));
  };

  const calcCost = (promptTokens: number, completionTokens: number, pricing?: Pricing) => {
    if (!pricing) return null;
    const promptCost = (promptTokens / 1000) * pricing.prompt_price;
    const completionCost = (completionTokens / 1000) * pricing.completion_price;
    return promptCost + completionCost;
  };

  const formatJson = (val: string | null | undefined) => {
    if (!val) return '(empty)';
    try {
      return JSON.stringify(JSON.parse(val), null, 2);
    } catch {
      return val;
    }
  };

  const formatResponse = (val: string | null | undefined, streaming: boolean) => {
    if (!val) return streaming ? '(streaming, no data captured)' : '(empty)';
    if (!streaming) {
      try {
        return JSON.stringify(JSON.parse(val), null, 2);
      } catch {
        return val;
      }
    }
    const lines = val.split('\n').filter(l => l.trim());
    const parsed: string[] = [];
    for (const line of lines) {
      const sse = line.replace(/^data:\s*/, '');
      if (sse === '[DONE]') continue;
      try {
        const d = JSON.parse(sse);
        const content = d.choices?.[0]?.delta?.content
          || d.choices?.[0]?.text
          || '';
        if (content) parsed.push(content);
      } catch {
        parsed.push(line);
      }
    }
    return parsed.length > 0 ? parsed.join('') : val;
  };

  const streaming = record ? isStreaming(record) : false;
  const matchedModel = record ? findModel(record.model) : undefined;
  const totalCost = record ? calcCost(record.prompt_tokens, record.completion_tokens, matchedModel?.pricing) : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[90vw] max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t('usage.detailTitle')}
            {record && (
              <span className="font-mono text-xs text-muted-foreground">
                {record.request_id.substring(0, 12)}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : record ? (
          <div className="space-y-4 min-w-0">
            {/* Meta info deck */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3 min-w-0">
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{t('table.user')}</div>
                <div className="font-medium truncate">{record.user_name}</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{t('table.model')}</div>
                <div className="font-medium truncate flex items-center gap-1.5">
                  {record.model}
                  {streaming ? (
                    <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-yellow-600 bg-yellow-50 dark:text-yellow-400 dark:bg-yellow-950 px-1.5 py-0.5 rounded">
                      <Radio className="h-2.5 w-2.5" />stream
                    </span>
                  ) : (
                    <span className="inline-flex items-center gap-0.5 text-[10px] font-medium text-blue-600 bg-blue-50 dark:text-blue-400 dark:bg-blue-950 px-1.5 py-0.5 rounded">
                      <RadioIcon className="h-2.5 w-2.5" />sync
                    </span>
                  )}
                </div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{t('table.status')}</div>
                <div className="flex items-center gap-1.5">
                  {record.success ? (
                    <CheckCircle2 className="h-4 w-4 text-green-500" />
                  ) : (
                    <XCircle className="h-4 w-4 text-red-500" />
                  )}
                  <span className="font-medium">{record.status_code}</span>
                </div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{t('table.latency')}</div>
                <div className="font-medium font-mono">{record.latency_ms}ms</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">Prompt tokens</div>
                <div className="font-medium font-mono">{record.prompt_tokens.toLocaleString()}</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">Completion tokens</div>
                <div className="font-medium font-mono">{record.completion_tokens.toLocaleString()}</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">Total tokens</div>
                <div className="font-medium font-mono">{record.total_tokens.toLocaleString()}</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">Cost</div>
                <div className="font-medium font-mono">
                  {totalCost !== null
                    ? `$${totalCost.toFixed(6)}`
                    : <span className="text-muted-foreground text-xs">—</span>
                  }
                </div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">Channel</div>
                <div className="font-medium font-mono text-xs truncate">{record.channel_id}</div>
              </div>
              <div className="rounded-lg border p-3 space-y-1">
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">{t('table.time')}</div>
                <div className="font-medium text-xs">{new Date(record.timestamp).toLocaleString()}</div>
              </div>
            </div>

            {/* Request body */}
            <div>
              <h4 className="text-sm font-medium mb-1">Request</h4>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-60 overflow-y-auto whitespace-pre-wrap break-all max-w-full">
                {formatJson(record.request_body)}
              </pre>
            </div>

            {/* Response body */}
            <div className="max-w-full">
              <h4 className="text-sm font-medium mb-1">
                Response
                {streaming && <span className="ml-2 text-xs text-yellow-500">(streaming)</span>}
              </h4>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-80 overflow-y-auto whitespace-pre-wrap break-all max-w-full">
                {formatResponse(record.response_body, streaming)}
              </pre>
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
