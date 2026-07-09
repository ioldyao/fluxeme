import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { usePublicModels, useSubscriptions, useSubscribeModel, useUnsubscribeModel } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Search, Check, Loader2, Cpu, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import type { Model } from '@/types';

function inferProvider(pattern: string): string {
  const p = pattern.toLowerCase();
  if (/^(gpt-|o[1-9]-|dall-e-|whisper-|tts-|text-)/.test(p)) return 'OpenAI';
  if (/^claude-/.test(p)) return 'Anthropic';
  if (/^gemini-/.test(p)) return 'Google';
  if (/^llama-/.test(p)) return 'Meta';
  if (/^deepseek-/.test(p)) return 'DeepSeek';
  if (/^mistral-/.test(p)) return 'Mistral';
  if (/^qwen/.test(p)) return 'Alibaba';
  if (/^yi-/.test(p)) return '01.AI';
  if (/^command-/.test(p)) return 'Cohere';
  if (/^flux-/.test(p)) return 'Black Forest';
  return '';
}

function inferCategory(pattern: string): string {
  const p = pattern.toLowerCase();
  if (/^(whisper-|tts-|audio-)/.test(p)) return 'audio';
  if (/^(dall-e-|flux-|stable-|sdxl)/.test(p)) return 'image';
  if (/^(text-embedding|embedding)/.test(p)) return 'embedding';
  if (/^(deepseek|o[1-9])/.test(p)) return 'reasoning';
  return 'chat';
}

const CATEGORY_LABELS: Record<string, string> = {
  chat: '对话',
  reasoning: '推理',
  image: '图像',
  embedding: '向量',
  audio: '语音',
};

const CATEGORY_COLORS: Record<string, string> = {
  chat: 'default',
  reasoning: 'success',
  image: 'warning',
  embedding: 'secondary',
  audio: 'muted',
};

const CATEGORY_KEYS = ['all', 'chat', 'reasoning', 'image', 'embedding', 'audio'] as const;
type CategoryKey = (typeof CATEGORY_KEYS)[number];

export default function ModelsMarketplace() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: models, isLoading, isError, refetch } = usePublicModels();
  const { data: subscriptions } = useSubscriptions();
  const subscribe = useSubscribeModel();
  const unsubscribe = useUnsubscribeModel();
  const [category, setCategory] = useState<CategoryKey>('all');
  const [query, setQuery] = useState('');

  const subscribedIds = useMemo(() => new Set(subscriptions?.map((m) => m.id) ?? []), [subscriptions]);

  const enriched = useMemo(() => {
    if (!models) return [];
    return models
      .map((m) => ({
        ...m,
        _provider: inferProvider(m.model_pattern),
        _category: inferCategory(m.model_pattern),
      }))
      .filter((m) => {
        if (category !== 'all' && m._category !== category) return false;
        if (!query) return true;
        const q = query.toLowerCase();
        return m.name.toLowerCase().includes(q) || m.model_pattern.toLowerCase().includes(q) || m._provider.toLowerCase().includes(q);
      });
  }, [models, category, query]);

  const handleToggle = (modelId: string, isSubscribed: boolean) => {
    const opts = {
      onSuccess: () => {
        toast.success(isSubscribed ? t('marketplace.unsubSuccess') : t('marketplace.subSuccess'));
        refetch();
        queryClient.invalidateQueries({ queryKey: ['me', 'subscriptions'] });
      },
      onError: (err: Error) => toast.error(err.message),
    };
    if (isSubscribed) {
      unsubscribe.mutate(modelId, opts);
    } else {
      subscribe.mutate(modelId, opts);
    }
  };

  const pending = subscribe.isPending || unsubscribe.isPending;

  return (
    <div className="space-y-6 animate-fade-in">
      <PageHeader
        title={t('marketplace.title')}
        description={t('marketplace.subtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      {/* Filters + Search */}
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div className="flex flex-wrap gap-1.5">
          {CATEGORY_KEYS.map((k) => (
            <button
              key={k}
              onClick={() => setCategory(k)}
              className={cn(
                'rounded-lg border px-3 py-1.5 text-sm font-medium transition-colors',
                category === k
                  ? 'border-primary bg-primary/10 text-primary'
                  : 'border-border text-muted-foreground hover:bg-muted hover:text-foreground',
              )}
            >
              {k === 'all' ? t('marketplace.all') : CATEGORY_LABELS[k]}
            </button>
          ))}
        </div>
        <div className="relative md:w-64">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('marketplace.search')}
            className="h-9 w-full rounded-lg border border-input bg-background pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
          />
        </div>
      </div>

      {/* Content */}
      {isLoading ? (
        <div className="p-12 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : isError ? (
        <div className="flex items-center justify-center p-8">
          <div className="text-center">
            <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
            <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
          </div>
        </div>
      ) : enriched.length > 0 ? (
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
          {enriched.map((model) => {
            const isSubscribed = subscribedIds.has(model.id);
            return (
              <ModelCard
                key={model.id}
                model={model}
                isSubscribed={isSubscribed}
                pending={pending}
                onToggle={handleToggle}
              />
            );
          })}
        </div>
      ) : (
        <EmptyState message={query ? t('marketplace.noMatch') : t('marketplace.noModels')} />
      )}
    </div>
  );
}

function formatPrice(price: number): string {
  if (!price || price === 0) return '-';
  return `$${price}`;
}

function formatContextLength(len: number | null | undefined): string {
  if (!len) return '-';
  if (len >= 1_000_000) return `${(len / 10_000).toFixed(0)} 万`;
  if (len >= 1_000) return `${(len / 1_000).toFixed(0)}K`;
  return len.toLocaleString();
}

function ModelCard({
  model,
  isSubscribed,
  pending,
  onToggle,
}: {
  model: Model & { _provider: string; _category: string };
  isSubscribed: boolean;
  pending: boolean;
  onToggle: (id: string, subscribed: boolean) => void;
}) {
  const { t } = useTranslation();

  return (
    <Card className="group flex flex-col transition-colors hover:border-primary/40">
      <CardContent className="flex flex-1 flex-col gap-4 p-5">
        {/* Header */}
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
              <Cpu className="size-5" />
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h3 className="font-semibold leading-none truncate">{model.name}</h3>
                {isSubscribed && <Badge variant="default" className="shrink-0">{t('marketplace.subscribed')}</Badge>}
              </div>
              <div className="flex items-center gap-2 mt-1">
                <p className="text-xs text-muted-foreground">{model._provider || t('marketplace.provider')}</p>
                {model._category && (
                  <Badge variant={CATEGORY_COLORS[model._category] as any} className="text-[10px] px-1.5 py-0">
                    {CATEGORY_LABELS[model._category]}
                  </Badge>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Pattern + Context Length */}
        <div className="flex items-center justify-between gap-2">
          <p className="text-xs font-mono text-muted-foreground bg-muted/50 rounded px-2 py-1 flex-1 truncate">
            {model.model_pattern}
          </p>
          {model.context_length != null && model.context_length > 0 && (
            <Badge variant="outline" className="shrink-0 text-[10px] px-1.5 py-0.5 gap-1">
              <span className="text-muted-foreground">{t('marketplace.contextLength')}</span>
              <span className="font-mono">{formatContextLength(model.context_length)}</span>
            </Badge>
          )}
        </div>

        {/* Pricing */}
        <div className="grid grid-cols-2 gap-2 border-t border-border pt-4 text-center">
          <div>
            <p className="text-xs text-muted-foreground">{t('marketplace.prompt')}</p>
            <p className="mt-0.5 text-sm font-medium">{formatPrice(model.pricing.prompt_price)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t('marketplace.completion')}</p>
            <p className="mt-0.5 text-sm font-medium">{formatPrice(model.pricing.completion_price)}</p>
          </div>
        </div>

        {/* Channels info */}
        {model.channels.length > 0 && (
          <p className="text-xs text-muted-foreground text-center">
            {t('marketplace.channels', { count: model.channels.length })}
          </p>
        )}

        {/* Action */}
        <Button
          variant={isSubscribed ? 'outline' : 'default'}
          size="sm"
          onClick={() => onToggle(model.id, isSubscribed)}
          disabled={pending}
          className="w-full mt-auto"
        >
          {isSubscribed ? (
            pending ? <Loader2 className="size-4 animate-spin" /> : <Check className="size-4 mr-1" />
          ) : null}
          {isSubscribed ? t('marketplace.subscribed') : t('marketplace.subscribe')}
        </Button>
      </CardContent>
    </Card>
  );
}
