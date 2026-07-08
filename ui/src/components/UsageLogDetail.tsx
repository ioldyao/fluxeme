import { useTranslation } from 'react-i18next';
import { useUsageDetail } from '@/api/usage';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { CheckCircle2, XCircle } from 'lucide-react';

interface Props {
  requestId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function UsageLogDetail({ requestId, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const { data: record, isLoading, error } = useUsageDetail(requestId);

  const isStreaming = (record: { request_body?: string | null }) => {
    if (!record.request_body) return false;
    try {
      const body = JSON.parse(record.request_body);
      return body.stream === true;
    } catch {
      return false;
    }
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
    // For SSE streaming data, try to extract meaningful content
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

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[90vw] max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {t('usage.detailTitle')}
            {record && (
              <span className="font-mono text-xs text-muted-foreground">
                {record.request_id.substring(0, 8)}
              </span>
            )}
          </DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : record ? (
          <div className="space-y-4">
            {/* Meta info */}
            <div className="grid grid-cols-2 gap-3 text-sm">
              <div><span className="text-muted-foreground">{t('table.user')}:</span> {record.user_name}</div>
              <div><span className="text-muted-foreground">{t('table.model')}:</span> {record.model}</div>
              <div><span className="text-muted-foreground">{t('table.time')}:</span> {new Date(record.timestamp).toLocaleString()}</div>
              <div><span className="text-muted-foreground">{t('table.status')}:</span>
                {record.success ? (
                  <CheckCircle2 className="h-4 w-4 text-green-500 inline ml-1" />
                ) : (
                  <XCircle className="h-4 w-4 text-red-500 inline ml-1" />
                )}
                <span className="ml-1">{record.status_code}</span>
              </div>
              <div><span className="text-muted-foreground">Prompt tokens:</span> {record.prompt_tokens}</div>
              <div><span className="text-muted-foreground">Completion tokens:</span> {record.completion_tokens}</div>
              <div><span className="text-muted-foreground">{t('table.latency')}:</span> {record.latency_ms}ms</div>
              <div><span className="text-muted-foreground">Channel:</span> {record.channel_id}</div>
            </div>

            {/* Request body */}
            <div>
              <h4 className="text-sm font-medium mb-1">Request</h4>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-60 overflow-y-auto">
                {formatJson(record.request_body)}
              </pre>
            </div>

            {/* Response body */}
            <div>
              <h4 className="text-sm font-medium mb-1">
                Response
                {isStreaming(record) && <span className="ml-2 text-xs text-yellow-500">(streaming)</span>}
              </h4>
              <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-80 overflow-y-auto whitespace-pre-wrap break-all">
                {formatResponse(record.response_body, isStreaming(record))}
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
