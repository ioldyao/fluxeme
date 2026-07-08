import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useModels, useDeleteModel, usePublishModel } from '@/api/models';
import { ModelForm } from '@/forms/ModelForm';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Pencil, Trash2, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { Model } from '@/types';

export default function Models() {
  const { t } = useTranslation();
  const { data: models, isLoading, refetch } = useModels();
  const deleteModel = useDeleteModel();
  const publishModel = usePublishModel();
  const [editModel, setEditModel] = useState<Model | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Model | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteModel.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('model.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('model.subtitle')}</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="h-4 w-4 mr-1" />{t('common.refresh')}
          </Button>
          <Button onClick={() => setShowAdd(true)}>
            <Plus className="h-4 w-4 mr-1" />{t('model.add')}
          </Button>
        </div>
      </div>
      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : models && models.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.id')}</th>
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.modelPattern')}</th>
                    <th className="text-right py-3 px-4">{t('table.bindings')}</th>
                    <th className="text-right py-3 px-4">{t('table.price')}</th>
                    <th className="text-center py-3 px-4">发布</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {models.map((m) => (
                    <tr key={m.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{m.id}</td>
                      <td className="py-3 px-4">{m.name}</td>
                      <td className="py-3 px-4 text-xs text-muted-foreground font-mono">{m.model_pattern}</td>
                      <td className="py-3 px-4 text-right">{m.channels.length}</td>
                      <td className="py-3 px-4 text-right text-xs">
                        P:{m.pricing.prompt_price} / C:{m.pricing.completion_price}
                      </td>
                      <td className="py-3 px-4 text-center">
                        <Button
                          variant={m.published ? "outline" : "secondary"}
                          size="sm"
                          className="h-7 text-xs"
                          onClick={() => publishModel.mutate(m.id, { onError: (err) => toast.error(err.message) })}
                          disabled={publishModel.isPending}
                        >
                          {m.published ? '已发布' : '发布'}
                        </Button>
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditModel(m)}>
                          <Pencil className="h-3 w-3" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(m)}>
                          <Trash2 className="h-3 w-3 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noModels')} />
          )}
        </CardContent>
      </Card>
      {(showAdd || editModel) && (
        <ModelForm
          model={editModel}
          open={true}
          onOpenChange={(open) => { if (!open) { setShowAdd(false); setEditModel(null); }}}
        />
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteModel')}${deleteTarget?.id}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
