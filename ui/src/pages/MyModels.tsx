import { useTranslation } from 'react-i18next';
import { useSubscriptions, useUnsubscribeModel } from '@/api/models';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { RefreshCw, Trash2, Loader2 } from 'lucide-react';
import { toast } from 'sonner';

export default function MyModels() {
  const { t } = useTranslation();
  const { data: models, isLoading, refetch } = useSubscriptions();
  const unsubscribe = useUnsubscribeModel();

  const handleUnsubscribe = (modelId: string) => {
    unsubscribe.mutate(modelId, {
      onSuccess: () => toast.success('已取消订阅'),
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">我的模型</h1>
          <p className="text-sm text-muted-foreground">管理你订阅的模型</p>
        </div>
        <Button variant="outline" size="sm" onClick={() => refetch()}>
          <RefreshCw className="h-4 w-4 mr-1" />{t('common.refresh')}
        </Button>
      </div>

      {isLoading ? (
        <div className="p-12 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : models && models.length > 0 ? (
        <div className="grid grid-cols-1 gap-3">
          {models.map((model) => (
            <Card key={model.id}>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <h3 className="font-medium">{model.name}</h3>
                      <span className="text-xs text-muted-foreground font-mono">{model.model_pattern}</span>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {model.channels.map((ch) => (
                        <Badge key={ch.channel_id} variant="secondary" className="text-xs">
                          {ch.channel_id} (优先级: {ch.priority})
                        </Badge>
                      ))}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      P: ${model.pricing.prompt_price}/1K · C: ${model.pricing.completion_price}/1K
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleUnsubscribe(model.id)}
                    disabled={unsubscribe.isPending}
                  >
                    {unsubscribe.isPending ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Trash2 className="h-4 w-4 text-destructive" />
                    )}
                  </Button>
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
