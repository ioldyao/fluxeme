import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { useUsageDetail } from '@fluxeme/shared/src/api/usage';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { useCurrency } from '@fluxeme/shared/src/store/currency';
import { parseTimestamp, formatTime } from '@fluxeme/shared/src/lib/date';
import { User, Brain, Reply } from 'lucide-react';
import type { UsageRecord } from '@fluxeme/shared/src/types';

interface Props {
  requestId: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface LifecycleEvent {
  cls: string;
  title: string;
  time: string;
  detail: string;
  durationMs?: number;
}

const COLORS: Record<string, string> = {
  ok: 'var(--chart-1)',
  pending: 'var(--chart-1)',
  streaming: 'var(--chart-1)',
  warn: 'var(--chart-1)',
  fail: 'var(--destructive)',
};

function formatDuration(ms: number) {
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}

function estimateEvents(record: UsageRecord, t: TFunction): LifecycleEvent[] {
  const ts = parseTimestamp(record.timestamp);
  const events: LifecycleEvent[] = [];
  let prevOffset = 0;

  const push = (cls: string, title: string, offsetMs: number, detail: string, showDuration = true) => {
    const ms = Math.max(0, Math.floor(offsetMs));
    events.push({
      cls,
      title,
      time: formatTime(new Date(ts.getTime() + ms)),
      detail,
      ...(showDuration ? { durationMs: ms - prevOffset } : {}),
    });
    prevOffset = ms;
  };

  // 1. Gateway Accepted
  push('ok', t('usage.lifecycleAccepted'), 0, t('usage.lifecycleAcceptedDetail'), false);

  const total = record.latency_ms;
  if (total > 0) {
    // 2. Auth & Route (estimated ~50ms)
    push('ok', t('usage.lifecycleAuthRoute'), 50, `${record.api_format ?? 'openai'} · ${record.channel_id}`);

    // 3. TTFT — real data from upstream, if available
    const ttft = record.ttft_ms && record.ttft_ms > 0 ? Math.floor(record.ttft_ms) : null;
    let startMs: number;

    if (ttft != null) {
      startMs = ttft;
      push('pending', t('usage.lifecycleTtft'), ttft, t('usage.lifecycleTtftDetail', { ms: ttft.toLocaleString() }));
    } else {
      startMs = Math.floor(total * 0.3);
    }

    if (record.success) {
      const tokenTotal = (record.prompt_tokens + record.cache_hit_input_tokens + record.completion_tokens).toLocaleString();

      // 4. Streaming Started / Provider Processing
      push(
        record.stream ? 'streaming' : 'ok',
        record.stream ? t('usage.lifecycleStreamingStarted') : t('usage.lifecycleProviderProcessing'),
        startMs,
        record.stream
          ? t('usage.lifecycleStreamingDetail', { n: record.completion_tokens.toLocaleString() })
          : t('usage.lifecycleProcessingDetail', { n: tokenTotal }),
      );

      // 5. Completed / Response Received
      push(
        'ok',
        record.stream ? t('usage.lifecycleCompleted') : t('usage.lifecycleResponseReceived'),
        total,
        t('usage.lifecycleStatusDetail', { code: String(record.status_code), ms: total.toLocaleString() }),
      );
    } else {
      push('fail', t('usage.lifecycleFailed'), total, t('usage.lifecycleStatusDetail', { code: String(record.status_code), ms: total.toLocaleString() }));
    }
  }

  return events;
}

function formatJson(val: string | null | undefined) {
  if (!val) return '(empty)';
  try { return JSON.stringify(JSON.parse(val), null, 2); } catch { return val; }
}

/** Strip Agent context markup (CLAUDE.md, system-reminder, command caveats). */
function stripAgentMarkup(text: string): string {
  return text
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/g, '')
    .replace(/<local-command-caveat>[\s\S]*?<\/local-command-caveat>/g, '')
    .replace(/<command-name>[\s\S]*?<\/command-message>/g, '')
    .trim();
}

/** Extract the latest user message text(s) from a conversation request body.
 *
 *  For Agent-style sessions (Claude Code, etc.) the `messages` array may hold
 *  hundreds of turns. We extract the **most recent real user input** —
 *  skipping empty messages, `<system-reminder>` markup, and system-injected
 *  "CRITICAL: Respond with TEXT ONLY" prompts. */
function extractUserMessages(body: string | null | undefined): string[] {
  if (!body) return [];
  let parsed: unknown;
  try { parsed = JSON.parse(body); } catch { return []; }
  const messages = (parsed as { messages?: unknown })?.messages;
  if (!Array.isArray(messages)) return [];
  const texts: string[] = [];

  for (const m of messages) {
    const msg = m as { role?: string; content?: unknown };
    if (msg.role !== 'user') continue;
    const c = msg.content;
    let text = '';

    if (typeof c === 'string') {
      text = c;
    } else if (Array.isArray(c)) {
      const parts: string[] = [];
      for (const block of c as Record<string, unknown>[]) {
        if (block.type === 'tool_result' || block.type === 'tool_use') continue;
        const tv = block.text;
        if (typeof tv === 'string' && tv.trim()) parts.push(tv);
      }
      text = parts.join('\n');
    }

    const stripped = stripAgentMarkup(text).trim();
    if (!stripped) continue;
    // Skip system-injected "respond with text only" prompts
    if (/^CRITICAL:/i.test(stripped)) continue;
    texts.push(stripped);
  }

  // Agent sessions may have hundreds of user turns — the relevant "user
  // request" is the most recent real user input.
  if (texts.length > 1) return [texts[texts.length - 1]];
  return texts;
}

/** Extract thinking + content from a response body.
 *
 *  Supports three formats:
 *  1. **Streaming (SSE)** — lines starting with `data: `.
 *  2. **Non-streaming JSON** — OpenAI choices[0].message or Anthropic content[].
 *  3. **Plain text** — content text saved directly by the stream handler
 *     (extract_sse_content's output). */
function extractResponseParts(body: string | null | undefined): { thinking: string; content: string } {
  if (!body) return { thinking: '', content: '' };
  const trimmed = body.trim();

  // 1. SSE streaming data
  if (trimmed.startsWith('data:')) {
    return extractSseParts(trimmed);
  }

  // 2. Try JSON parse
  try {
    const parsed = JSON.parse(trimmed) as Record<string, unknown>;
    // Anthropic non-streaming: content[] blocks
    if (Array.isArray(parsed.content)) {
      let thinking = '';
      let content = '';
      for (const block of parsed.content as { type?: string; text?: string; thinking?: string }[]) {
        if (block?.type === 'text' && typeof block.text === 'string') content += block.text;
        if (block?.type === 'thinking' && typeof block.thinking === 'string') thinking += block.thinking;
      }
      if (content || thinking) return { thinking, content };
    }
    // OpenAI non-streaming: choices[0].message
    const msg = (parsed.choices as { message?: { content?: unknown; reasoning_content?: unknown } }[] | undefined)?.[0]?.message;
    if (msg) {
      return {
        thinking: typeof msg.reasoning_content === 'string' ? msg.reasoning_content : '',
        content: typeof msg.content === 'string' ? msg.content : '',
      };
    }
    // Error shape: { error: { message } }
    if (parsed.error && typeof (parsed.error as Record<string, unknown>).message === 'string') {
      return { thinking: '', content: (parsed.error as Record<string, unknown>).message as string };
    }
  } catch { /* fall through to plain text fallback */ }

  // 3. Plain text — the stream handler saves extract_sse_content output directly.
  //    This catches novels, thinking-only responses, and raw content.
  return { thinking: '', content: trimmed };
}

/** Parse reasoning/content out of SSE chunks (mirrors backend extract_sse_content). */
function extractSseParts(data: string): { thinking: string; content: string } {
  let thinking = '';
  let content = '';
  for (const line of data.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed || trimmed === 'data: [DONE]' || trimmed.startsWith('event: ')) continue;
    const jsonStr = trimmed.startsWith('data: ') ? trimmed.slice(6) : trimmed;
    let val: Record<string, unknown>;
    try { val = JSON.parse(jsonStr); } catch { continue; }
    // OpenAI: choices[0].delta.{reasoning_content, reasoning, content}
    const openaiDelta = (val.choices as { delta?: Record<string, unknown> }[] | undefined)?.[0]?.delta;
    if (openaiDelta) {
      const rt = openaiDelta.reasoning_content ?? openaiDelta.reasoning;
      if (typeof rt === 'string' && rt) thinking += rt;
      if (typeof openaiDelta.content === 'string' && openaiDelta.content) content += openaiDelta.content;
    }
    // Anthropic: content_block_delta delta.{thinking, text}
    const blockDelta = val.delta as { type?: string; thinking?: string; text?: string } | undefined;
    if (val.type === 'content_block_delta' && blockDelta) {
      if (blockDelta.type === 'thinking_delta' && typeof blockDelta.thinking === 'string') thinking += blockDelta.thinking;
      if (blockDelta.type === 'text_delta' && typeof blockDelta.text === 'string') content += blockDelta.text;
    }
  }
  return { thinking, content };
}

export function UsageLogDetail({ requestId, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const { data: record, isLoading, error } = useUsageDetail(requestId);
  useCurrency();

  const userMessages = record ? extractUserMessages(record.request_body) : [];
  const { thinking: respThinking, content: respContent } = record
    ? extractResponseParts(record.response_body)
    : { thinking: '', content: '' };
  const thinkingText = (record?.reasoning_body?.trim() || respThinking.trim()).trim() || null;
  // When the stream handler saved a thinking-only plain-text response, it may
  // duplicate reasoning_body — don't show it as the reply as well.
  let replyText = respContent.trim() || null;
  if (replyText && thinkingText && replyText === thinkingText) {
    replyText = null;
  }

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
                { label: t('table.model'), value: record.original_model ? `${record.original_model} → ${record.model}` : record.model },
                { label: t('usage.apiKey'), value: record.api_key_name ?? '—' },
                { label: t('usage.keyScope'), value: record.team_id ? t('usage.teamKey') : t('usage.personalKey') },
                { label: t('usage.apiFormat'), value: record.api_format ?? '—' },
                { label: t('usage.channel'), value: record.channel_id },
                { label: t('usage.clientIp'), value: record.client_ip ?? '—' },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-medium truncate">{m.value}</div>
                </div>
              ))}
            </div>

            {/* Token facts row */}
            <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
              {[
                { label: t('usage.uncachedInput'), value: record.prompt_tokens.toLocaleString() },
                { label: t('usage.cachedInput'), value: record.cache_hit_input_tokens > 0 ? record.cache_hit_input_tokens.toLocaleString() : '—' },
                { label: t('dash.completion'), value: record.completion_tokens.toLocaleString() },
              ].map(m => (
                <div key={m.label} className="rounded-lg border bg-card p-3">
                  <div className="text-[10px] font-medium text-muted-foreground tracking-wider mb-1">{m.label}</div>
                  <div className="text-sm font-semibold font-mono">{m.value}</div>
                </div>
              ))}
            </div>

            <hr className="border-border" />

            {/* Request Lifecycle Timeline */}
            <div>
              <h3 className="text-sm font-semibold mb-3 flex items-center gap-2">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
                {t('usage.requestLifecycle')}
              </h3>
              <div className="grid grid-cols-1 xl:grid-cols-[1.2fr_0.65fr] gap-4">
                {/* Timeline */}
                <div className="relative pl-[34px]">
                  <div className="absolute left-[10px] top-[8px] bottom-[8px] w-[2px] bg-border" />
                  {estimateEvents(record, t).map((ev, i) => (
                    <div key={i} className="relative pb-4 last:pb-0">
                      <div className="absolute left-[-29px] top-[3px] w-[12px] h-[12px] rounded-full bg-card border-[3px]" style={{ borderColor: COLORS[ev.cls] || COLORS.ok }} />
                      <div className="flex justify-between gap-3">
                        <div className="font-semibold text-sm">{ev.title}</div>
                        <div className="flex items-center gap-1.5 shrink-0">
                          <span className="text-[11px] font-mono text-muted-foreground">{ev.time}</span>
                          {ev.durationMs != null && ev.durationMs > 0 && (
                            <span className="text-[10px] text-muted-foreground/60 font-mono">
                              {t('usage.lifecycleStageDuration', { duration: formatDuration(ev.durationMs) })}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="mt-1 text-xs text-muted-foreground">{ev.detail}</div>
                    </div>
                  ))}
                </div>

                {/* Inspector panel */}
                <div className="rounded-lg border bg-card p-4">
                  <h4 className="text-sm font-semibold mb-2">{t('usage.detailTitle')}</h4>
                  <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium ${record.success ? 'bg-chart-2/10 text-chart-2' : 'bg-destructive/10 text-destructive'}`}>
                    <span className={`size-1.5 rounded-full ${record.success ? 'bg-chart-2' : 'bg-destructive'}`} />
                    {record.success ? t('usage.success') : t('usage.failure')} · HTTP {record.status_code}
                  </span>
                  <div className="mt-3 space-y-0">
                    {[
                      [t('table.status'), `${record.success ? t('usage.success') : t('usage.failure')}`],
                      [t('table.requestId'), record.request_id],
                      [t('table.latency'), `${record.latency_ms}ms`],
                      [t('usage.totalTokens'), (record.prompt_tokens + record.cache_hit_input_tokens + record.completion_tokens).toLocaleString()],
                    ].map((r, i) => (
                      <div key={i} className="flex justify-between gap-3 py-2 border-t border-border/60 first:border-0">
                        <span className="text-xs text-muted-foreground">{r[0]}</span>
                        <b className="text-xs text-right break-all max-w-[200px]">{r[1]}</b>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>

            <hr className="border-border" />

            {/* Conversation: User Request → Thinking → Reply */}
            <div className="space-y-4">
              {/* User request */}
              <div>
                <h4 className="text-sm font-medium mb-2 flex items-center gap-1.5">
                  <User className="size-4 text-muted-foreground" />{t('usage.userRequest')}
                </h4>
                {userMessages.length > 0 ? (
                  <div className="space-y-2">
                    {userMessages.map((text, i) => (
                      <div key={i} className="rounded-lg border bg-muted/40 p-3 text-xs whitespace-pre-wrap break-words">{text}</div>
                    ))}
                  </div>
                ) : (
                  <div className="text-xs text-muted-foreground">{t('usage.noUserMessage')}</div>
                )}
                {record.request_body && (
                  <details className="mt-2">
                    <summary className="text-[11px] text-muted-foreground cursor-pointer select-none">{t('usage.requestRaw')}</summary>
                    <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-60 overflow-y-auto whitespace-pre-wrap break-all mt-1">{formatJson(record.request_body)}</pre>
                  </details>
                )}
              </div>

              {/* Thinking */}
              {thinkingText && (
                <div>
                  <h4 className="text-sm font-medium mb-2 flex items-center gap-1.5">
                    <Brain className="size-4 text-muted-foreground" />{t('usage.thinking')}
                  </h4>
                  <div className="rounded-lg border bg-muted/30 p-3 text-xs italic text-muted-foreground whitespace-pre-wrap break-words">{thinkingText}</div>
                </div>
              )}

              {/* Reply */}
              <div>
                <h4 className="text-sm font-medium mb-2 flex items-center gap-1.5">
                  <Reply className="size-4 text-muted-foreground" />{t('usage.reply')}
                </h4>
                {replyText ? (
                  <div className="rounded-lg border bg-chart-2/5 p-3 text-xs whitespace-pre-wrap break-words">{replyText}</div>
                ) : (
                  <div className="text-xs text-muted-foreground">—</div>
                )}
                {record.response_body && (
                  <details className="mt-2">
                    <summary className="text-[11px] text-muted-foreground cursor-pointer select-none">{t('usage.responseRaw')}</summary>
                    <pre className="rounded-lg bg-muted p-3 text-xs overflow-x-auto max-h-80 overflow-y-auto whitespace-pre-wrap break-all mt-1">{formatJson(record.response_body)}</pre>
                  </details>
                )}
              </div>
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