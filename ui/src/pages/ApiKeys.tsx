import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useApiKeys, useCreateApiKey, useUpdateApiKey, useDeleteApiKey, useSaveApiKey } from '@/api/apiKeys';
import { useCurrency, CURRENCY_SYMBOL } from '@/store/currency';
import { ApiKeyForm } from '@/forms/ApiKeyForm';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { CopyButton } from '@/components/CopyButton';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Plus, Pencil, Trash2, RefreshCw } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { toast } from 'sonner';
import type { ApiKey, CreateKeyReq } from '@/types';

export default function ApiKeys() {
  const { t } = useTranslation();
  const { currency } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];
  const { data: keys, isLoading, isError, refetch } = useApiKeys();
  const createKey = useCreateApiKey();
  const deleteKey = useDeleteApiKey();
  const updateKey = useUpdateApiKey();
  const saveKey = useSaveApiKey();
  const [showAdd, setShowAdd] = useState(false);
  const [editKey, setEditKey] = useState<ApiKey | null>(null);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteKey.mutate(deleteTarget.key, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleEditSubmit = (data: CreateKeyReq) => {
    if (!editKey) return;
    saveKey.mutate(
      { keyVal: editKey.key, data },
      {
        onSuccess: () => { toast.success('已更新'); setEditKey(null); refetch(); },
        onError: (err) => toast.error(err.message),
      },
    );
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('apikey.title')}
        description={t('apikey.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('apikey.add')}
            </Button>
          </>
        }
      />
      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : isError ? (
            <div className="flex items-center justify-center p-8">
              <div className="text-center">
                <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
                <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
              </div>
            </div>
          ) : keys && keys.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.key')}</th>
                    <th className="text-center py-3 px-4">{t('table.statusLabel')}</th>
                    <th className="text-left py-3 px-4">{t('apikey.expires')}</th>
                    <th className="text-right py-3 px-4">费用限制</th>
                    <th className="text-left py-3 px-4">模型限制</th>
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
                        <Switch
                          checked={k.enabled}
                          onCheckedChange={() =>
                            updateKey.mutate({ keyVal: k.key, enabled: !k.enabled }, {
                              onError: (err) => toast.error(err.message),
                            })
                          }
                          disabled={updateKey.isPending}
                        />
                      </td>
                      <td className="py-3 px-4 text-xs text-muted-foreground">
                        {k.expires_at ? new Date(k.expires_at).toLocaleDateString() : t('apikey.never')}
                      </td>
                      <td className="py-3 px-4 text-right text-xs">
                        {k.spend_limit != null ? `${sym}${k.spend_limit}` : '-'}
                      </td>
                      <td className="py-3 px-4 text-xs text-muted-foreground max-w-[150px] truncate">
                        {k.allowed_models && k.allowed_models.length > 0 ? k.allowed_models.join(', ') : '-'}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditKey(k)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(k)}>
                          <Trash2 className="size-3.5 text-destructive" />
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
          onOpenChange={(open) => { if (!open) { setShowAdd(false); setCreatedKey(null); }}}
          createdKey={createdKey}
          onSubmit={(data: any) => {
            createKey.mutate(data, {
              onSuccess: (resp: any) => {
                setCreatedKey(resp.key);
                toast.success(t('apikey.generatedTitle'));
              },
              onError: (err) => toast.error(err.message),
            });
          }}
          isPending={createKey.isPending}
        />
      )}
      {editKey && (
        <ApiKeyForm
          open={true}
          editKey={editKey}
          onOpenChange={(open) => { if (!open) setEditKey(null); }}
          onSubmit={handleEditSubmit}
          isPending={saveKey.isPending}
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
