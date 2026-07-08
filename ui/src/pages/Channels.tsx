import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useChannels, useDeleteChannel } from '@/api/channels';
import { ChannelForm } from '@/forms/ChannelForm';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Pencil, Trash2, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { Channel } from '@/types';

export default function Channels() {
  const { t } = useTranslation();
  const { data: channels, isLoading, refetch } = useChannels();
  const deleteChannel = useDeleteChannel();
  const [editChannel, setEditChannel] = useState<Channel | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Channel | null>(null);

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteChannel.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('channel.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('channel.subtitle')}</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="h-4 w-4 mr-1" />{t('common.refresh')}
          </Button>
          <Button onClick={() => setShowAdd(true)}>
            <Plus className="h-4 w-4 mr-1" />{t('channel.add')}
          </Button>
        </div>
      </div>
      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : channels && channels.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.id')}</th>
                    <th className="text-left py-3 px-4">{t('table.provider')}</th>
                    <th className="text-center py-3 px-4">{t('table.priority')}</th>
                    <th className="text-center py-3 px-4">{t('table.endpoints')}</th>
                    <th className="text-center py-3 px-4">{t('table.statusLabel')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {channels.map((ch) => (
                    <tr key={ch.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{ch.id}</td>
                      <td className="py-3 px-4 capitalize">{ch.provider}</td>
                      <td className="py-3 px-4 text-center">{ch.priority}</td>
                      <td className="py-3 px-4 text-center">{ch.endpoints.length}</td>
                      <td className="py-3 px-4 text-center">
                        <Badge variant={ch.enabled ? 'default' : 'secondary'}>
                          {ch.enabled ? t('common.active') : t('common.disabled')}
                        </Badge>
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditChannel(ch)}>
                          <Pencil className="h-3 w-3" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(ch)}>
                          <Trash2 className="h-3 w-3 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noChannels')} />
          )}
        </CardContent>
      </Card>
      {(showAdd || editChannel) && (
        <ChannelForm
          channel={editChannel}
          open={true}
          onOpenChange={(open) => { if (!open) { setShowAdd(false); setEditChannel(null); }}}
        />
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteChannel')}${deleteTarget?.id}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
