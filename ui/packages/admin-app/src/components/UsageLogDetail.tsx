import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { useUsageDetail, useUsageRequestDetail, useUsageRequestAttempts, type UsageRequest } from '@fluxeme/shared/src/api/usage';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { parseTimestamp, formatTime } from '@fluxeme/shared/src/lib/date';
import { useState } from 'react';
import { Copy, Check, Clock3 } from 'lucide-react';
import type { UsageRecord } from '@fluxeme/shared/src/types';
import type { UsageRequestAttempt } from '@fluxeme/shared/src/api/usage';

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
  /** Anchor absolute epoch ms (for sorting/relative durations). */
  anchorMs: number;
}

/** Real, persisted attempt start times keyed by attempt number, when the
 *  attempt event carries a valid timestamp. Falls back to a request-time
 *  offset so legacy data still renders in order. */
function attemptStartMs(attempts: UsageRequestAttempt[] | undefined, requestTsMs: number, fallbackBaseMs: number): Map<number, number> {
  const map = new Map<number, number>();
  if (!attempts || attempts.length === 0) return map;
  let fallback = fallbackBaseMs;
  for (const a of attempts) {
    const completed = a.timestamp ? parseTimestamp(a.timestamp) : null;
    const completedMs = completed && !isNaN(completed.getTime()) ? completed.getTime() : fallback;
    // Attempt events persist their terminal timestamp; subtracting the
    // measured duration reconstructs the real upstream start boundary.
    const ms = Math.max(requestTsMs, completedMs - Math.max(0, a.latency_ms || 0));
    map.set(a.attempt_no, ms);
    fallback = completedMs + Math.max(1, a.latency_ms || 1);
  }
  return map;
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

/** Build the full request lifecycle timeline from the request fact, its
 *  upstream attempts, and the channel name map. Absolute times are used when
 *  the attempt events carry them; otherwise a request-relative estimate keeps
 *  legacy data readable. Every attempt surfaces the channel and endpoint it
 *  actually hit, so a retried request shows each route it took. */
function buildLifecycle(
  record: UsageRecord,
  request: UsageRequest | null | undefined,
  t: TFunction,
  attempts: UsageRequestAttempt[] | undefined,
  channelNameById: Map<string, string>,
): LifecycleEvent[] {
  const terminalTs = parseTimestamp(record.timestamp);
  // Gateway request events persist their terminal timestamp. Reconstruct the
  // request start boundary from the measured total latency.
  const ts = new Date(terminalTs.getTime() - Math.max(0, record.latency_ms || 0));
  const events: LifecycleEvent[] = [];
  let prev = 0;

  const push = (
    cls: string,
    title: string,
    offsetMs: number,
    detail: string,
    explicitDurationMs?: number,
  ) => {
    const ms = Math.max(0, Math.floor(offsetMs));
    const durationMs = explicitDurationMs != null ? explicitDurationMs : ms - prev;
    events.push({
      cls,
      title,
      time: formatTime(new Date(ts.getTime() + ms)),
      detail,
      ...(durationMs > 0 ? { durationMs } : {}),
      anchorMs: ms,
    });
    prev = ms;
  };

  const channelLabel = (id?: string | null) => {
    if (!id) return '—';
    const name = channelNameById.get(id);
    return name && name !== id ? `${name} (${id})` : id;
  };
  const routeDetail = (a?: UsageRequestAttempt) => {
    const parts = [a ? channelLabel(a.channel_id) : channelLabel(record.channel_id)];
    const ep = a ? a.endpoint_url : record.endpoint_url;
    const endpointId = a ? a.endpoint_id : record.endpoint_id;
    if (ep) parts.push(`${ep}${endpointId != null ? ` (#${endpointId})` : ''}`);
    else if (endpointId != null) parts.push(`endpoint #${endpointId}`);
    return parts.filter(Boolean).join(' · ');
  };

  // 1. Gateway Accepted
  push('ok', t('usage.lifecycleAccepted'), 0, t('usage.lifecycleAcceptedDetail'));

  const total = record.latency_ms;
  if (total <= 0) return events;

  // 2. Auth & Route (estimated ~50ms)
  push('ok', t('usage.lifecycleAuthRoute'), 50, `${record.api_format ?? 'openai'} · ${record.channel_id ?? '—'}`);

  // 3. Route resolved — the channel and endpoint this request was routed to.
  if (record.channel_id || record.endpoint_url) {
    push('ok', t('usage.lifecycleRouteResolved'), 50, routeDetail());
  }

  if (record.original_model && record.original_model !== record.model && request?.model_mapping_rule) {
    push('ok', t('usage.lifecycleModelMapping'), 51, `${record.original_model}\n↓\n${record.model}\n${t('usage.lifecycleMatchedRule')}: ${request.model_mapping_rule}`);
  }

  // 4. Upstream attempts — each shows its own channel + endpoint + result.
  const attemptBaseMs = attemptStartMs(attempts, ts.getTime(), ts.getTime() + 50);
  const hasAttempts = attempts && attempts.length > 0;
  if (hasAttempts) {
    for (const a of attempts!) {
    const startOffset = Math.max(50, attemptStartMsOf(a, attemptBaseMs, ts.getTime()) - ts.getTime());
    const statusText = a.success ? 'HTTP ' + (a.status_code ?? 200) : (a.error || 'failed');
      push(
        a.success ? 'ok' : 'fail',
        t('usage.lifecycleAttempt', { n: a.attempt_no }),
        startOffset,
        `${routeDetail(a)} · ${statusText}`,
        Math.max(0, a.latency_ms || 0),
      );
    }
  }

  // 5. TTFT — real data from upstream, if available
  const ttft = record.ttft_ms && record.ttft_ms > 0 ? Math.floor(record.ttft_ms) : null;
  const hasAttemptsOrStream = true;
  void hasAttemptsOrStream;
  let outputMs: number;

  if (ttft != null) {
    outputMs = Math.max(prev, ttft);
    push('pending', t('usage.lifecycleTtft'), outputMs, t('usage.lifecycleTtftDetail', { ms: ttft.toLocaleString() }));
  } else {
    // Estimate streaming start after the last attempt (or 30% of total when
    // no attempt timing exists).
    outputMs = hasAttempts ? Math.max(prev, Math.floor(total * 0.3)) : Math.floor(total * 0.3);
  }

  if (record.success) {
    const tokenTotal = (record.prompt_tokens + record.cache_hit_input_tokens + record.completion_tokens).toLocaleString();

    // 6. Streaming Started / Provider Processing
    push(
      record.stream ? 'streaming' : 'ok',
      record.stream ? t('usage.lifecycleStreamingStarted') : t('usage.lifecycleProviderProcessing'),
      outputMs,
      record.stream
        ? t('usage.lifecycleStreamingDetail', { n: record.completion_tokens.toLocaleString() })
        : t('usage.lifecycleProcessingDetail', { n: tokenTotal }),
    );

    // 7. Completed / Response Received
    push(
      'ok',
      record.stream ? t('usage.lifecycleCompleted') : t('usage.lifecycleResponseReceived'),
      total,
      t('usage.lifecycleStatusDetail', { code: String(record.status_code), ms: total.toLocaleString() }),
    );
  } else {
    push('fail', t('usage.lifecycleFailed'), total, t('usage.lifecycleStatusDetail', { code: String(record.status_code), ms: total.toLocaleString() }));
  }

  return events;
}

function attemptStartMsOf(a: UsageRequestAttempt, base: Map<number, number>, requestTsMs: number): number {
  const v = base.get(a.attempt_no);
  if (v != null) return v;
  return requestTsMs + 50;
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
  const { data: request, isLoading, error } = useUsageRequestDetail(requestId);
  const { data: legacyRecord } = useUsageDetail(requestId);
  const record: UsageRecord | null = request ? {
    ...request,
    model: request.resolved_model || request.requested_model,
    original_model: request.resolved_model && request.resolved_model !== request.requested_model ? request.requested_model : '',
    stream: request.stream !== 0,
    success: request.status === 'succeeded',
    latency_ms: request.total_latency_ms,
    cache_hit_input_tokens: request.cache_read_tokens,
    // Request events intentionally do not store payload bodies. Reuse the
    // legacy usage fact when one exists so successful calls retain their
    // request/reply inspection; rejected requests correctly remain empty.
    request_body: legacyRecord?.request_body ?? null,
    response_body: legacyRecord?.response_body ?? null,
    reasoning_body: legacyRecord?.reasoning_body ?? null,
    api_key_name: request.api_key_name ?? null,
    client_ip: request.client_ip ?? null,
    endpoint_id: request.endpoint_id ?? null,
    endpoint_url: request.endpoint_url ?? null,
    team_id: request.team_id ?? null,
    ttft_ms: request.ttft_ms ?? null,
    billing_group_id: null,
    billing_group_name: null,
    billing_payment_mode: request.billing_payment_mode ?? null,
    account_type: null,
    prompt_price: legacyRecord?.prompt_price ?? 0,
    completion_price: legacyRecord?.completion_price ?? 0,
    cache_read_price: legacyRecord?.cache_read_price ?? 0,
    cache_write_price: legacyRecord?.cache_write_price ?? 0,
  } as UsageRecord : null;
  const { data: attempts } = useUsageRequestAttempts(requestId);
  const { data: channels } = useChannels(undefined, { enabled: open });
  const channelNameById = new Map((channels ?? []).map(channel => [channel.id, channel.name]));
  const [copied, setCopied] = useState(false);
  const [tab, setTab] = useState<'request' | 'response' | 'thinking' | 'raw'>('request');
  const copyRequestId = async () => {
    if (!record) return;
    await navigator.clipboard?.writeText(record.request_id);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  const channelName = record ? (channelNameById.get(record.channel_id) ?? record.channel_id) : '—';
  const totalTokens = record ? record.prompt_tokens + record.cache_hit_input_tokens + record.completion_tokens : 0;
  const formattedTotalTokens = totalTokens.toLocaleString();
  const lifecycleEvents = record ? buildLifecycle(record, request, t, attempts, channelNameById) : [];
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
          <div className="mx-auto max-w-[1600px] space-y-5 min-w-0">
            {/* Request conclusion */}
            <div className="rounded-xl border bg-card px-5 py-4">
              <div className="flex flex-wrap items-start justify-between gap-4">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h2 className="text-lg font-semibold">{t('usage.detailTitle')}</h2>
                    <button type="button" onClick={copyRequestId} className="inline-flex items-center gap-1 rounded-md border px-2 py-1 font-mono text-[11px] text-muted-foreground hover:bg-muted" title={record.request_id}>
                      {record.request_id.substring(0, 18)}… {copied ? <Check className="size-3 text-chart-2" /> : <Copy className="size-3" />}
                    </button>
                  </div>
                  <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-muted-foreground">
                    <span className="font-medium text-foreground">{record.original_model && record.original_model !== record.model ? `${record.original_model} → ${record.model}` : record.model}</span><span>·</span><span>{record.api_format ?? '—'}</span><span>·</span><span>{record.user_name}</span><span>·</span><span>{record.api_key_name ?? '—'}</span>
                  </div>
                </div>
                <div className="flex items-center gap-3 text-right">
                  <span className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium ${record.success ? 'bg-chart-2/10 text-chart-2' : 'bg-destructive/10 text-destructive'}`}>
                    <span className={`size-1.5 rounded-full ${record.success ? 'bg-chart-2' : 'bg-destructive'}`} />{record.success ? t('usage.success') : t('usage.failure')} {record.status_code}
                  </span>
                  <div><div className="font-mono text-lg font-semibold">{formatDuration(record.latency_ms)}</div><div className="text-[10px] text-muted-foreground">{t('table.latency')}</div></div>
                </div>
              </div>
            </div>

            {/* KPI metrics */}
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-5">
              {[
                { label: t('usage.totalTokens'), value: formattedTotalTokens },
                { label: t('usage.uncachedInput'), value: record.prompt_tokens.toLocaleString() },
                { label: t('usage.cachedInput'), value: record.cache_hit_input_tokens > 0 ? record.cache_hit_input_tokens.toLocaleString() : '—' },
                { label: t('dash.completion'), value: record.completion_tokens.toLocaleString() },
                { label: t('usage.attemptCount'), value: String(request?.attempt_count ?? attempts?.length ?? 0) },
              ].map(m => <div key={m.label} className="rounded-lg border bg-card px-4 py-3"><div className="text-[10px] font-medium tracking-wider text-muted-foreground">{m.label}</div><div className="mt-1 font-mono text-base font-semibold">{m.value}</div></div>)}
            </div>

            {/* Trace + context */}
            <div className="grid grid-cols-1 gap-5 xl:grid-cols-[minmax(0,1.5fr)_minmax(280px,0.8fr)]">
              <section className="rounded-xl border bg-card p-5">
                <h3 className="mb-4 flex items-center gap-2 text-sm font-semibold"><Clock3 className="size-4" />{t('usage.requestLifecycle')}</h3>
                <div className="relative pl-7">
                  <div className="absolute bottom-2 left-[7px] top-2 w-px bg-border" />
                  {lifecycleEvents.map((ev, i) => <div key={i} className="relative pb-5 last:pb-0">
                    <div className="absolute -left-[27px] top-1 size-3 rounded-full border-[3px] bg-card" style={{ borderColor: COLORS[ev.cls] || COLORS.ok }} />
                    <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1"><div className="text-sm font-semibold">{ev.title}</div><div className="flex items-center gap-2 font-mono text-[11px] text-muted-foreground"><span>{ev.time}</span>{ev.durationMs != null && ev.durationMs > 0 && <span className="text-muted-foreground/70">+{formatDuration(ev.durationMs)}</span>}</div></div>
                    <div className="mt-1 break-words text-xs text-muted-foreground">{ev.detail}</div>
                  </div>)}
                </div>
                {request && request.status !== 'succeeded' && (request.error_kind || request.error_stage || request.error_message) && <div className="mt-5 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs"><div className="font-semibold text-destructive">{request.status} · HTTP {request.status_code}</div>{request.error_stage && <div className="mt-1 text-muted-foreground">{t('usage.errorStage')} {request.error_stage}</div>}{request.error_kind && <div className="text-muted-foreground">{t('usage.errorKind')} {request.error_kind}</div>}{request.error_message && <div className="break-words text-muted-foreground">{t('usage.errorMessage')} {request.error_message}</div>}</div>}
              </section>

              <aside className="space-y-4">
                <section className="rounded-xl border bg-card p-4"><h4 className="mb-3 text-sm font-semibold">{t('usage.requestInfo')}</h4><div className="space-y-2 text-xs">
                  {[[t('table.user'),record.user_name],[t('usage.apiKey'),record.api_key_name ?? '—'],[t('usage.keyScope'),record.team_id ? t('usage.teamKey') : t('usage.personalKey')],[t('usage.apiFormat'),record.api_format ?? '—'],[t('usage.endpointPath'),record.path ?? '—'],[t('usage.clientIp'),record.client_ip ?? '—']].map(([k,v]) => <div key={k} className="flex justify-between gap-3 border-b border-border/60 pb-2 last:border-0"><span className="text-muted-foreground">{k}</span><b className="max-w-[65%] break-all text-right">{v}</b></div>)}
                </div></section>
                <section className="rounded-xl border bg-card p-4"><h4 className="mb-3 text-sm font-semibold">{t('usage.routeInfo')}</h4><div className="space-y-2 text-xs">
                  {[[t('usage.requestedModel'),request?.requested_model ?? record.model],[t('usage.mappedModel'),record.model],...(request?.model_mapping_rule ? [[t('usage.mappingRule'), request.model_mapping_rule] as [string, string]] : []),[t('usage.channel'),channelName],[t('usage.channelId'),record.channel_id ?? '—'],[t('usage.endpointId'),record.endpoint_id != null ? `#${record.endpoint_id}` : '—'],['Endpoint',record.endpoint_url ?? '—']].map(([k,v]) => <div key={k} className="flex justify-between gap-3 border-b border-border/60 pb-2 last:border-0"><span className="text-muted-foreground">{k}</span><b className="max-w-[65%] break-all text-right">{v}</b></div>)}
                </div></section>
                <section className="rounded-xl border bg-card p-4"><h4 className="mb-3 text-sm font-semibold">{t('usage.billingMode')}</h4><div className="flex justify-between text-xs"><span className="text-muted-foreground">{t('usage.billingMode')}</span><b>{record.billing_payment_mode === 'prepaid' ? t('usage.prepaid') : t('usage.metered')}</b></div></section>
              </aside>
            </div>

            <hr className="border-border" />

            {/* Request content (tabs) */}
            <section className="rounded-xl border bg-card p-5">
              <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                <h3 className="text-sm font-semibold">{t('usage.requestContent')}</h3>
                <div className="flex flex-wrap items-center gap-1 rounded-lg bg-muted p-0.5 text-xs">
                  {([['request', t('usage.requestContentRequest')], ['response', t('usage.requestContentResponse')], ['thinking', t('usage.thinking')], ['raw', t('usage.requestContentRaw')]] as const).map(([key, label]) => <button key={key} type="button" onClick={() => setTab(key)} className={`rounded-md px-2.5 py-1 transition-colors ${tab === key ? 'bg-card font-medium shadow-sm' : 'text-muted-foreground'}`}>{label}</button>)}
                </div>
              </div>

              {tab === 'request' && (
                <div className="space-y-2">
                  {userMessages.length > 0 ? userMessages.map((text, i) => <div key={i} className="whitespace-pre-wrap break-words rounded-lg border bg-muted/40 p-3 text-xs">{text}</div>) : <div className="text-xs text-muted-foreground">{t('usage.noUserMessage')}</div>}
                  {record.request_body && <details className="mt-2"><summary className="cursor-pointer select-none text-[11px] text-muted-foreground">{t('usage.requestRaw')}</summary><pre className="mt-1 max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-muted p-3 text-xs">{formatJson(record.request_body)}</pre></details>}
                </div>
              )}
              {tab === 'response' && (
                <div className="space-y-2">
                  {replyText ? <div className="whitespace-pre-wrap break-words rounded-lg border bg-chart-2/5 p-3 text-xs">{replyText}</div> : <div className="text-xs text-muted-foreground">—</div>}
                  {record.response_body && <details className="mt-2"><summary className="cursor-pointer select-none text-[11px] text-muted-foreground">{t('usage.responseRaw')}</summary><pre className="mt-1 max-h-80 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-muted p-3 text-xs">{formatJson(record.response_body)}</pre></details>}
                </div>
              )}
              {tab === 'thinking' && (
                thinkingText ? <div className="whitespace-pre-wrap break-words rounded-lg border bg-muted/30 p-3 text-xs italic text-muted-foreground">{thinkingText}</div> : <div className="text-xs text-muted-foreground">—</div>
              )}
              {tab === 'raw' && (
                <div className="space-y-2">
                  {[[t('usage.requestRaw'), record.request_body],[t('usage.responseRaw'), record.response_body],[t('usage.reasoningRaw'), record.reasoning_body]].filter(([,v]) => v).map(([label, val]) => <div key={label as string}><div className="mb-1 text-[11px] font-medium text-muted-foreground">{label}</div><pre className="max-h-96 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-muted p-3 text-xs">{formatJson(val as string | null | undefined)}</pre></div>)}
                </div>
              )}
            </section>
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