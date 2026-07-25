import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useMyRules, useCreateMyRule, useDeleteMyRule } from '@/api/rules';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Trash2, Plus, RefreshCw, ArrowRight } from 'lucide-react';
import { toast } from 'sonner';

export default function MyRules() {
  const { t } = useTranslation();
  const { data: rules, isLoading, isError, refetch } = useMyRules();
  const createRule = useCreateMyRule();
  const deleteRule = useDeleteMyRule();
  const [showAdd, setShowAdd] = useState(false);
  const [sourceModel, setSourceModel] = useState('');
  const [targetModel, setTargetModel] = useState('');

  const handleCreate = () => {
    if (!sourceModel || !targetModel) {
      toast.error('请填写来源模型和目标模型');
      return;
    }
    createRule.mutate(
      { source_model: sourceModel, target_model: targetModel },
      {
        onSuccess: () => {
          toast.success('规则已创建');
          setShowAdd(false);
          setSourceModel('');
          setTargetModel('');
          refetch();
        },
        onError: (err) => toast.error(err.message),
      },
    );
  };

  const handleDelete = (id: string) => {
    deleteRule.mutate(id, {
      onSuccess: () => { toast.success('已删除'); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title="我的路由规则"
        description="自定义你想要的模型请求应该转发到哪个模型"
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />添加规则
            </Button>
          </>
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
      ) : rules && rules.length > 0 ? (
        <div className="space-y-2">
          {rules.map((rule) => (
            <Card key={rule.id}>
              <CardContent className="p-4">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <code className="px-2 py-1 rounded bg-muted font-mono text-sm">{rule.source_model}</code>
                    <ArrowRight className="size-4 text-muted-foreground" />
                    <code className="px-2 py-1 rounded bg-primary/10 font-mono text-sm text-primary">{rule.target_model}</code>
                  </div>
                  <Button variant="ghost" size="sm" onClick={() => handleDelete(rule.id)}>
                    <Trash2 className="size-4 text-destructive" />
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      ) : (
        <EmptyState message="还没有自定义规则。添加规则后，你发送的模型名会被自动转发到你指定的目标模型。" />
      )}

      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>添加路由规则</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>来源模型</Label>
              <Input
                value={sourceModel}
                onChange={(e) => setSourceModel(e.target.value)}
                placeholder="gpt-4"
              />
              <p className="text-xs text-muted-foreground">你发出的模型名（精确匹配）</p>
            </div>
            <div className="space-y-2">
              <Label>目标模型</Label>
              <Input
                value={targetModel}
                onChange={(e) => setTargetModel(e.target.value)}
                placeholder="claude-sonnet-4"
              />
              <p className="text-xs text-muted-foreground">实际转发到的模型名</p>
            </div>
            <Button onClick={handleCreate} className="w-full" disabled={createRule.isPending}>
              {createRule.isPending ? '创建中...' : '创建'}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
