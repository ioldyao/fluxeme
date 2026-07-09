import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
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
  if (/^qwen-/.test(p)) return 'Alibaba';
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
  const { data: models, isLoading, refetch } = usePublicModels();
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
        toast.success(isSubscribed ? '已取消订阅' : '订阅成功');
        refetch();
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
        title="模型广场"
        description="浏览并订阅已发布的模型"
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
              {k === 'all' ? '全部' : CATEGORY_LABELS[k]}
            </button>
          ))}
        </div>
        <div className="relative md:w-64">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索模型名称或厂商…"
            className="h-9 w-full rounded-lg border border-input bg-background pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
          />
        </div>
      </div>

      {/* Content */}
      {isLoading ? (
        <div className="p-12 text-center text-muted-foreground">{t('common.loading')}</div>
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
        <EmptyState message={query ? '没有找到匹配的模型' : '暂无已发布的模型'} />
      )}
    </div>
  );
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
                {isSubscribed && <Badge variant="default" className="shrink-0">已订阅</Badge>}
              </div>
              <div className="flex items-center gap-2 mt-1">
                <p className="text-xs text-muted-foreground">{model._provider || '未分类'}</p>
                {model._category && (
                  <Badge variant={CATEGORY_COLORS[model._category] as any} className="text-[10px] px-1.5 py-0">
                    {CATEGORY_LABELS[model._category]}
                  </Badge>
                )}
              </div>
            </div>
          </div>
        </div>

        {/* Pattern */}
        <p className="text-xs font-mono text-muted-foreground bg-muted/50 rounded px-2 py-1">
          {model.model_pattern}
        </p>

        {/* Pricing */}
        <div className="grid grid-cols-2 gap-2 border-t border-border pt-4 text-center">
          <div>
            <p className="text-xs text-muted-foreground">Prompt / 1K</p>
            <p className="mt-0.5 text-sm font-medium">${model.pricing.prompt_price}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Completion / 1K</p>
            <p className="mt-0.5 text-sm font-medium">${model.pricing.completion_price}</p>
          </div>
        </div>

        {/* Channels info */}
        {model.channels.length > 0 && (
          <p className="text-xs text-muted-foreground text-center">
            {model.channels.length} 个渠道绑定
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
          ) : (
            '订阅'
          )}
          {isSubscribed ? '已订阅' : ''}
        </Button>
      </CardContent>
    </Card>
  );
}
