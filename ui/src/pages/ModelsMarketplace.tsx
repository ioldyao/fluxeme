import { useTranslation } from 'react-i18next';
import { usePublicModels, useSubscriptions, useSubscribeModel, useUnsubscribeModel } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { RefreshCw, Check, Plus, Loader2 } from 'lucide-react';
import { toast } from 'sonner';

export default function ModelsMarketplace() {
  const { t } = useTranslation();
  const { data: models, isLoading, refetch } = usePublicModels();
  const { data: subscriptions } = useSubscriptions();
  const subscribe = useSubscribeModel();
  const unsubscribe = useUnsubscribeModel();

  const subscribedIds = new Set(subscriptions?.map((m) => m.id) ?? []);

  const handleToggle = (modelId: string, isSubscribed: boolean) => {
    if (isSubscribed) {
      unsubscribe.mutate(modelId, {
        onSuccess: () => toast.success('已取消订阅'),
        onError: (err) => toast.error(err.message),
      });
    } else {
      subscribe.mutate(modelId, {
        onSuccess: () => toast.success('订阅成功'),
        onError: (err) => toast.error(err.message),
      });
    }
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title="模型广场"
        description="浏览并订阅已发布的模型"
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      {isLoading ? (
        <div className="p-12 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : models && models.length > 0 ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {models.map((model) => {
            const isSubscribed = subscribedIds.has(model.id);
            const pending = subscribe.isPending || unsubscribe.isPending;
            return (
              <Card key={model.id} className="flex flex-col">
                <CardHeader>
                  <div className="flex items-start justify-between">
                    <div className="space-y-1">
                      <CardTitle>{model.name}</CardTitle>
                      <p className="text-xs text-muted-foreground font-mono">{model.model_pattern}</p>
                    </div>
                    {isSubscribed && <Badge>已订阅</Badge>}
                  </div>
                </CardHeader>
                <CardContent className="flex-1 flex flex-col justify-between gap-3">
                  <div className="space-y-1 text-xs text-muted-foreground">
                    <div className="flex justify-between">
                      <span>Prompt 价格</span>
                      <span className="font-mono">${model.pricing.prompt_price}/1K</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Completion 价格</span>
                      <span className="font-mono">${model.pricing.completion_price}/1K</span>
                    </div>
                  </div>
                  <Button
                    variant={isSubscribed ? "outline" : "default"}
                    size="sm"
                    onClick={() => handleToggle(model.id, isSubscribed)}
                    disabled={pending}
                    className="w-full"
                  >
                    {isSubscribed ? (
                      pending ? <Loader2 className="size-4 animate-spin" /> : <Check className="size-4 mr-1" />
                    ) : (
                      <Plus className="size-4 mr-1" />
                    )}
                    {isSubscribed ? '已订阅' : '订阅'}
                  </Button>
                </CardContent>
              </Card>
            );
          })}
        </div>
      ) : (
        <EmptyState message="暂无已发布的模型" />
      )}
    </div>
  );
}
