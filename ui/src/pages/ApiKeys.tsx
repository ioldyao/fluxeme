import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useApiKeys, useDeleteApiKey } from '@/api/apiKeys';
import { ApiKeyForm } from '@/forms/ApiKeyForm';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { CopyButton } from '@/components/CopyButton';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Plus, Trash2, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { ApiKey } from '@/types';

export default function ApiKeys() {
  const { t } = useTranslation();
  const { data: keys, isLoading, refetch } = useApiKeys();
  const deleteKey = useDeleteApiKey();
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteKey.mutate(deleteTarget.key, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('apikey.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('apikey.subtitle')}</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="h-4 w-4 mr-1" />{t('common.refresh')}
          </Button>
          <Button onClick={() => setShowAdd(true)}>
            <Plus className="h-4 w-4 mr-1" />{t('apikey.add')}
          </Button>
        </div>
      </div>
      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : keys && keys.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.key')}</th>
                    <th className="text-center py-3 px-4">{t('table.statusLabel')}</th>
                    <th className="text-left py-3 px-4">{t('apikey.expires')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {keys.map((k) => (
                    <tr key={k.key} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4">{k.name || '-'}</td>
                      <td className="py-3 px-4">
                        <div className="flex items-center gap-2">
                          <code className="text-xs font-mono">{k.key.substring(0, 12)}...</code>
                          <CopyButton text={k.key} />
                        </div>
                      </td>
                      <td className="py-3 px-4 text-center">
                        <Badge variant={k.enabled ? 'default' : 'secondary'}>
                          {k.enabled ? t('common.active') : t('common.disabled')}
                        </Badge>
                      </td>
                      <td className="py-3 px-4 text-xs text-muted-foreground">
                        {k.expires_at ? new Date(k.expires_at).toLocaleDateString() : t('apikey.never')}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(k)}>
                          <Trash2 className="h-3 w-3 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noApiKeys')} />
          )}
        </CardContent>
      </Card>
      {showAdd && (
        <ApiKeyForm
          open={true}
          onOpenChange={(open) => { if (!open) setShowAdd(false); }}
        />
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteApiKey')}${deleteTarget?.name}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
