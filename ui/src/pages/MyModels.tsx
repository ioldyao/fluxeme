import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSubscriptions, useUnsubscribeModel, useTestModelConnection, type ModelTestResult } from '@/api/models';
import { useProbeResults } from '@/api/probe';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { RefreshCw, Trash2, Loader2, Link2 } from 'lucide-react';
import { toast } from 'sonner';
import { CURRENCY_SYMBOL, usePricingCurrency, useCurrency } from '@/store/currency';

const CATEGORY_ORDER = ['chat', 'reasoning', 'tools', 'web', 'vision', 'rerank', 'embedding'];

function fmtPerK(price: number): string {
  if (!price) return '0';
  return String(price);
}

export default function MyModels() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useSubscriptions();
  const unsubscribe = useUnsubscribeModel();
  const testConnection = useTestModelConnection();
  const [testingIds, setTestingIds] = useState<Record<string, boolean>>({});
  const [results, setResults] = useState<Record<string, ModelTestResult>>({});
  const { data: probeResults } = useProbeResults();
  const { currency } = useCurrency();
  const { effectiveCurrency: getEffectiveCurrency } = usePricingCurrency();

  const formatCtx = (v: number | null | undefined) => {
    if (!v) return null;
    if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
    if (v >= 1_000) return `${(v / 1_000).toFixed(0)}K`;
    return v.toLocaleString();
  };

  const handleUnsubscribe = (modelId: string) => {
    unsubscribe.mutate(modelId, {
      onSuccess: () => { toast.success('已取消订阅'); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleTestConnection = (modelId: string) => {
    setTestingIds((prev) => ({ ...prev, [modelId]: true }));
    testConnection.mutate(modelId, {
      onSuccess: (res) => {
        setTestingIds((prev) => ({ ...prev, [modelId]: false }));
        setResults((prev) => ({ ...prev, [modelId]: res }));
        if (res.success) {
          toast.success(`连接成功 (${res.latency_ms}ms)`);
        } else {
          toast.error(res.error || '连接失败');
        }
      },
      onError: (err) => {
        setTestingIds((prev) => ({ ...prev, [modelId]: false }));
        toast.error(err.message);
      },
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title="我的模型"
        description="管理你订阅的模型"
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      {isLoading ? (
        <div className="p-12 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : isError ? (
        <div className="flex items-center justify-center p-8">
          <div className="text-center">
            <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
            <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
          </div>
        </div>
      ) : models && models.length > 0 ? (
        <div className="grid grid-cols-1 gap-3">
          {models.map((model) => (
            <Card key={model.id}>
              <CardContent className="p-5">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      {(() => {
                        const r = results[model.id];
                        if (r) return <span className={`inline-block size-2 rounded-full ${r.success ? 'bg-green-500' : 'bg-red-500'}`} />;
                        const chId = model.channels?.[0]?.channel_id;
                        const pr = chId ? probeResults?.find((p) => p.channel_id === chId) : undefined;
                        if (pr) return <span className={`inline-block size-2 rounded-full ${pr.success ? 'bg-green-500' : 'bg-red-500'}`} />;
                        return null;
                      })()}
                      <h3
                        className="font-medium cursor-pointer hover:text-brand transition-colors"
                        onClick={() => {
                          const text = model.name;
                          if (navigator.clipboard) {
                            navigator.clipboard.writeText(text);
                          } else {
                            const el = document.createElement('textarea');
                            el.value = text;
                            document.body.appendChild(el);
                            el.select();
                            document.execCommand('copy');
                            document.body.removeChild(el);
                          }
                          toast.success(`已复制: ${text}`);
                        }}
                      >{model.name}</h3>
                      <span className="text-xs text-muted-foreground font-mono">{model.model_pattern}</span>
                      {(() => {
                        const r = results[model.id];
                        if (r?.latency_ms !== undefined) return <span className={`text-xs ${r.success ? 'text-green-600' : 'text-red-500'}`}>{r.latency_ms}ms</span>;
                        const chId = model.channels?.[0]?.channel_id;
                        const pr = chId ? probeResults?.find((p) => p.channel_id === chId) : undefined;
                        if (pr?.latency_ms !== undefined) return <span className={`text-xs ${pr.success ? 'text-green-600' : 'text-red-500'}`}>{pr.latency_ms}ms</span>;
                        return null;
                      })()}
                    </div>
                    {(model.category || model.context_length) && (
                      <div className="flex items-center gap-2 pt-0.5">
                        {model.category?.split(',').filter(Boolean).sort((a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b)).map((cat) => (
                          <span key={cat} className="inline-block px-1.5 py-0.5 text-[10px] font-medium rounded bg-muted text-muted-foreground">
                            {t(`model.category.${cat}`, { defaultValue: cat })}
                          </span>
                        ))}
                        {formatCtx(model.context_length) && (
                          <span className="text-[10px] font-mono text-muted-foreground">
                            {t('model.contextLabel')} {formatCtx(model.context_length)}
                          </span>
                        )}
                      </div>
                    )}
                    <div className="text-xs text-muted-foreground">
                      {(() => {
                        const sym = CURRENCY_SYMBOL[getEffectiveCurrency(currency, model.id)];
                        const parts = [
                          `${t('pricing.inputLabel')} ${sym}${fmtPerK(model.pricing.prompt_price)} / ${t('pricing.outputLabel')} ${sym}${fmtPerK(model.pricing.completion_price)}`
                        ];
                        if (model.pricing.cache_read_price > 0) {
                          parts.push(`${t('pricing.cacheLabel')} ${sym}${fmtPerK(model.pricing.cache_read_price)}`);
                        }
                        return parts.join(' · ');
                      })()}
                    </div>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleTestConnection(model.id)}
                      disabled={testingIds[model.id]}
                      title="测试连接"
                    >
                      {testingIds[model.id] ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <Link2 className="size-4" />
                      )}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleUnsubscribe(model.id)}
                      disabled={unsubscribe.isPending}
                    >
                      {unsubscribe.isPending ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <Trash2 className="size-4 text-destructive" />
                      )}
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <EmptyState message="你还没有订阅任何模型，去模型广场看看吧" />
      )}
    </div>
  );
}
