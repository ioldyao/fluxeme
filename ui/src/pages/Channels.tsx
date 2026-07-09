import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient, useMutation } from '@tanstack/react-query';
import { useChannels, useCreateChannel, useUpdateChannel, useDeleteChannel } from '@/api/channels';
import { api } from '@/api/client';
import { ChannelForm } from '@/forms/ChannelForm';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Pencil, Trash2, Plus, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import type { Channel } from '@/types';

export default function Channels() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const { data: channels, isLoading, refetch } = useChannels();
  const createChannel = useCreateChannel();
  const deleteChannel = useDeleteChannel();
  const [editChannel, setEditChannel] = useState<Channel | null>(null);
  const updateChannel = useUpdateChannel(editChannel?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Channel | null>(null);

  const toggleEnabled = useMutation({
    mutationFn: (ch: Channel) =>
      api<Channel>(`/channels/${encodeURIComponent(ch.id)}`, {
        method: 'PUT',
        body: { ...ch, enabled: !ch.enabled },
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['channels'] }),
    onError: (err) => toast.error(err.message),
  });

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteChannel.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('channel.title')}
        description={t('channel.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('channel.add')}
            </Button>
          </>
        }
      />
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
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
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
                      <td className="py-3 px-4">{ch.name || ch.id}</td>
                      <td className="py-3 px-4 capitalize">{ch.provider}</td>
                      <td className="py-3 px-4 text-center">{ch.priority}</td>
                      <td className="py-3 px-4 text-center">{ch.endpoints.length}</td>
                      <td className="py-3 px-4 text-center">
                        <Button
                          variant={ch.enabled ? 'outline' : 'secondary'}
                          size="sm"
                          className={cn('h-7 text-xs', ch.enabled ? 'text-green-600 border-green-300' : 'text-muted-foreground')}
                          onClick={() => toggleEnabled.mutate(ch)}
                          disabled={toggleEnabled.isPending}
                        >
                          {ch.enabled ? t('common.active') : t('common.disabled')}
                        </Button>
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditChannel(ch)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(ch)}>
                          <Trash2 className="size-3.5 text-destructive" />
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
          onSubmit={(data: any) => {
            if (editChannel) {
              updateChannel.mutate(data, {
                onSuccess: () => { toast.success(t('toast.updated')); setEditChannel(null); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            } else {
              createChannel.mutate(data, {
                onSuccess: () => { toast.success(t('toast.created')); setShowAdd(false); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            }
          }}
          isPending={createChannel.isPending || updateChannel.isPending}
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
