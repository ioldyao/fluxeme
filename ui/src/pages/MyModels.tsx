import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSubscriptions, useUnsubscribeModel, useTestModelConnection } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { RefreshCw, Trash2, Loader2, Link2 } from 'lucide-react';
import { toast } from 'sonner';

export default function MyModels() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useSubscriptions();
  const unsubscribe = useUnsubscribeModel();
  const testConnection = useTestModelConnection();
  const [testingId, setTestingId] = useState<string | null>(null);

  const handleUnsubscribe = (modelId: string) => {
    unsubscribe.mutate(modelId, {
      onSuccess: () => { toast.success('已取消订阅'); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleTestConnection = (modelId: string) => {
    setTestingId(modelId);
    testConnection.mutate(modelId, {
      onSuccess: (res) => {
        setTestingId(null);
        if (res.success) {
          toast.success('连接成功');
        } else {
          toast.error(res.error || '连接失败');
        }
      },
      onError: (err) => {
        setTestingId(null);
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
                      <h3 className="font-medium">{model.name}</h3>
                      <span className="text-xs text-muted-foreground font-mono">{model.model_pattern}</span>
                    </div>
                    <div className="text-xs text-muted-foreground">
                      P: ${model.pricing.prompt_price}/1K · C: ${model.pricing.completion_price}/1K
                    </div>
                  </div>
                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleTestConnection(model.id)}
                      disabled={testingId === model.id}
                      title="测试连接"
                    >
                      {testingId === model.id ? (
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
