import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { usePublicModels, useSubscriptions, useSubscribeModel, useUnsubscribeModel } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { ModelDetailDialog } from '@/components/ModelDetailDialog';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Search, Check, Loader2, Cpu, RefreshCw, Info } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import type { Model } from '@/types';

function inferProvider(pattern: string): string {
  const p = pattern.toLowerCase();
  if (/^(gpt-|o[1-9]-|dall-e-|whisper-|tts-|text-|realtime-)/.test(p)) return 'OpenAI';
  if (/^claude-/.test(p)) return 'Anthropic';
  if (/^gemini-/.test(p)) return 'Gemini';
  if (/^llama-/.test(p)) return 'Meta';
  if (/^deepseek-/.test(p)) return 'DeepSeek';
  if (/^mistral-/.test(p)) return 'Mistral';
  if (/^qwen/.test(p)) return 'Qwen';
  if (/^glm/.test(p)) return 'Zhipu';
  if (/^kimi-/.test(p)) return 'Kimi';
  if (/^yi-/.test(p)) return '01.AI';
  if (/^command-/.test(p)) return 'Cohere';
  if (/^flux-/.test(p)) return 'Black Forest';
  return '';
}

const CATEGORY_KEYS = ['chat', 'reasoning', 'tools', 'web', 'vision', 'rerank', 'embedding'] as const;

const PROVIDER_ICON: Record<string, string> = {
  OpenAI: 'openai',
  Anthropic: 'anthropic',
  Gemini: 'gemini-color',
  Meta: 'meta-color',
  DeepSeek: 'deepseek-color',
  Mistral: 'mistral-color',
  Qwen: 'qwen-color',
  Zhipu: 'zhipu-color',
  Kimi: 'kimi-color',
  '01.AI': 'zeroone-color',
  Cohere: 'cohere-color',
  'Black Forest': 'bfl',
};

const ICON_BASE = '/icons';

export default function ModelsMarketplace() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: models, isLoading, isError, refetch } = usePublicModels();
  const { data: subscriptions } = useSubscriptions();
  const subscribe = useSubscribeModel();
  const unsubscribe = useUnsubscribeModel();
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [author, setAuthor] = useState<string | null>(null);
  const [serviceProvider, setServiceProvider] = useState<string | null>(null);
  const [modality, setModality] = useState<string | null>(null);
  const [query, setQuery] = useState('');

  const subscribedIds = useMemo(() => new Set(subscriptions?.map((m) => m.id) ?? []), [subscriptions]);

  const authors = useMemo(() => {
    if (!models) return [];
    return [...new Set(models.map((m) => inferProvider(m.model_pattern)).filter(Boolean))].sort();
  }, [models]);

  const enriched = useMemo(() => {
    if (!models) return [];
    return models
      .map((m) => ({
        ...m,
        _provider: inferProvider(m.model_pattern),
      }))
      .filter((m) => {
        if (author && m._provider !== author) return false;
        if (serviceProvider && m._provider !== serviceProvider) return false;
        if (modality && !(m.category?.split(',').includes(modality) ?? false)) return false;
        if (!query) return true;
        const q = query.toLowerCase();
        return m.name.toLowerCase().includes(q) || m.model_pattern.toLowerCase().includes(q) || m._provider.toLowerCase().includes(q);
      });
  }, [models, author, serviceProvider, modality, query]);

  const handleToggle = (modelId: string, isSubscribed: boolean) => {
    setPendingId(modelId);
    const opts = {
      onSuccess: () => {
        setPendingId(null);
        toast.success(isSubscribed ? t('marketplace.unsubSuccess') : t('marketplace.subSuccess'));
        refetch();
        queryClient.invalidateQueries({ queryKey: ['me', 'subscriptions'] });
      },
      onError: (err: Error) => {
        setPendingId(null);
        toast.error(err.message);
      },
    };
    if (isSubscribed) {
      unsubscribe.mutate(modelId, opts);
    } else {
      subscribe.mutate(modelId, opts);
    }
  };

  const isPending = (id: string) => pendingId === id;

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

      <div className="flex gap-8">
        {/* Left sidebar — filters */}
        <aside className="w-56 shrink-0 space-y-5">
          <h3 className="text-sm font-semibold">{t('marketplace.categories')}</h3>

          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('marketplace.search')}
              className="h-9 w-full rounded-lg border border-input bg-background pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
            />
          </div>

          <div className="space-y-2">
            <h4 className="text-xs font-medium text-muted-foreground">{t('marketplace.author')}</h4>
            <div className="flex flex-wrap gap-1">
              <FilterBtn active={!author} onClick={() => setAuthor(null)} label={t('marketplace.all')} />
              {authors.map((a) => (
                <FilterBtn key={a} active={author === a} onClick={() => setAuthor(a)} label={a} />
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <h4 className="text-xs font-medium text-muted-foreground">{t('marketplace.inferenceProvider')}</h4>
            <div className="flex flex-wrap gap-1">
              <FilterBtn active={!serviceProvider} onClick={() => setServiceProvider(null)} label={t('marketplace.all')} />
              {authors.map((a) => (
                <FilterBtn key={a} active={serviceProvider === a} onClick={() => setServiceProvider(a)} label={a} />
              ))}
            </div>
          </div>

          <div className="space-y-2">
            <h4 className="text-xs font-medium text-muted-foreground">{t('marketplace.modality')}</h4>
            <div className="flex flex-wrap gap-1">
              <FilterBtn active={!modality} onClick={() => setModality(null)} label={t('marketplace.all')} />
              {CATEGORY_KEYS.map((k) => (
                <FilterBtn key={k} active={modality === k} onClick={() => setModality(k)} label={t(`model.category.${k}`)} />
              ))}
            </div>
          </div>
        </aside>

        {/* Right content — model cards */}
        <div className="flex-1 min-w-0">
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
            <div className="grid grid-cols-1 md:grid-cols-2 2xl:grid-cols-3 gap-4">
              {enriched.map((model) => {
                const isSubscribed = subscribedIds.has(model.id);
                return (
                  <ModelCard
                    key={model.id}
                    model={model}
                    isSubscribed={isSubscribed}
                    pending={isPending(model.id)}
                    onToggle={handleToggle}
                  />
                );
              })}
            </div>
          ) : (
            <EmptyState message={query ? t('marketplace.noMatch') : t('marketplace.noModels')} />
          )}
        </div>
      </div>
    </div>
  );
}

function FilterBtn({ active, onClick, label }: { active: boolean; onClick: () => void; label: string }) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'rounded-lg border px-2.5 py-1 text-xs font-medium transition-colors',
        active
          ? 'border-primary bg-primary/10 text-primary'
          : 'border-border text-muted-foreground hover:bg-muted hover:text-foreground',
      )}
    >
      {label}
    </button>
  );
}

function formatPrice(price: number): string {
  if (!price || price === 0) return '-';
  return `$${price}`;
}

function formatContextLength(len: number | null | undefined): string {
  if (!len) return '-';
  if (len >= 1_000_000) return `${(len / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
  if (len >= 1_000) return `${(len / 1_000).toFixed(0)}K`;
  return len.toLocaleString();
}

function ModelCard({
  model,
  isSubscribed,
  pending,
  onToggle,
}: {
  model: Model & { _provider: string };
  isSubscribed: boolean;
  pending: boolean;
  onToggle: (id: string, subscribed: boolean) => void;
}) {
  const { t } = useTranslation();
  const [detailOpen, setDetailOpen] = useState(false);

  return (
    <Card className="group flex flex-col transition-colors hover:border-primary/40">
      <CardContent className="flex flex-1 flex-col gap-4 p-5">
        {/* Header */}
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-center gap-3 min-w-0">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted">
              {PROVIDER_ICON[model._provider] ? (
                <img
                  src={`${ICON_BASE}/${PROVIDER_ICON[model._provider]}.svg`}
                  alt={model._provider}
                  className="size-6"
                />
              ) : (
                <Cpu className="size-5 text-muted-foreground" />
              )}
            </div>
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <h3 className="font-semibold leading-none truncate">{model.name}</h3>
                {isSubscribed && <Badge variant="default" className="shrink-0">{t('marketplace.subscribed')}</Badge>}
              </div>
              <div className="flex items-center gap-2 mt-1">
                <p className="text-xs text-muted-foreground">{model._provider || t('marketplace.provider')}</p>
                {(model.category?.split(',').filter(Boolean).sort((a, b) => CATEGORY_KEYS.indexOf(a as any) - CATEGORY_KEYS.indexOf(b as any)) ?? []).map((cat) => (
                  <span key={cat} className="inline-block px-1.5 py-0.5 text-[10px] font-medium rounded bg-muted text-muted-foreground">
                    {t(`model.category.${cat}`, { defaultValue: cat })}
                  </span>
                ))}
              </div>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="shrink-0 size-7 p-0"
            onClick={() => setDetailOpen(true)}
            title="详情"
          >
            <Info className="size-4" />
          </Button>
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
        <div className={`grid gap-2 border-t border-border pt-4 text-center ${model.pricing.cache_read_price > 0 ? 'grid-cols-3' : 'grid-cols-2'}`}>
          <div>
            <p className="text-xs text-muted-foreground">{t('marketplace.prompt')}</p>
            <p className="mt-0.5 text-sm font-medium">{formatPrice(model.pricing.prompt_price)}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t('marketplace.completion')}</p>
            <p className="mt-0.5 text-sm font-medium">{formatPrice(model.pricing.completion_price)}</p>
          </div>
          {model.pricing.cache_read_price > 0 && (
            <div>
              <p className="text-xs text-muted-foreground">{t('pricing.cacheRead')}</p>
              <p className="mt-0.5 text-sm font-medium">{formatPrice(model.pricing.cache_read_price)}</p>
            </div>
          )}
        </div>

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

      <ModelDetailDialog
        model={model}
        provider={model._provider}
        open={detailOpen}
        onOpenChange={setDetailOpen}
      />
    </Card>
  );
}
