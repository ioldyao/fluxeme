import { useState, Fragment } from 'react';
import { useTranslation } from 'react-i18next';
import { useQueryClient, useMutation } from '@tanstack/react-query';
import { useChannels, useCreateChannel, useUpdateChannel, useDeleteChannel } from '@fluxeme/shared/src/api/channels';
import { useChannelHealth, type EndpointHealthItem } from '@fluxeme/shared/src/api/balancer';
import { api } from '@fluxeme/shared/src/api/client';
import { ChannelForm } from '@/forms/ChannelForm';
import { PROVIDER_DISPLAY } from '@fluxeme/shared/src/constants/providers';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import { Pencil, Trash2, Plus, RefreshCw, ChevronRight } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@fluxeme/shared/src/lib/utils';
import type { Channel, Endpoint } from '@fluxeme/shared/src/types';

type EndpointStatus = 'healthy' | 'degraded' | 'circuitBroken' | 'disabled' | 'unused' | 'unknown';

function getEndpointStatus(ep: Endpoint, health: EndpointHealthItem[] | undefined): EndpointStatus {
  if (!health) return 'unknown';
  const item = health.find((h) => h.endpoint_id === ep.id);
  if (!item) return 'unknown';
  if (!item.enabled) return 'disabled';
  if (item.total_bindings === 0) return 'unused';
  if (item.long_unavailable) return 'circuitBroken';
  if (item.healthy_bindings === 0) return 'circuitBroken';
  if (item.healthy_bindings < item.total_bindings) return 'degraded';
  return 'healthy';
}

const STATUS_VARIANT: Record<EndpointStatus, 'default' | 'destructive' | 'secondary' | 'outline'> = {
  healthy: 'default',
  degraded: 'secondary',
  circuitBroken: 'destructive',
  disabled: 'secondary',
  unused: 'outline',
  unknown: 'outline',
};

function StatusBadge({ status }: { status: EndpointStatus }) {
  const { t } = useTranslation();
  const labels: Record<EndpointStatus, string> = {
    healthy: t('endpoint.statusHealthy'),
    degraded: t('endpoint.statusDegraded'),
    circuitBroken: t('endpoint.statusCircuitBroken'),
    disabled: t('endpoint.statusDisabled'),
    unused: t('endpoint.statusUnused'),
    unknown: t('endpoint.unknown'),
  };
  return <Badge variant={STATUS_VARIANT[status]}>{labels[status]}</Badge>;
}

// ── Expandable row — manages its own expanded state + health query ──

function ChannelRow({
  ch,
  onEdit,
  onDelete,
  toggleEnabled,
  togglePending,
}: {
  ch: Channel;
  onEdit: (ch: Channel) => void;
  onDelete: (ch: Channel) => void;
  toggleEnabled: (ch: Channel) => void;
  togglePending: boolean;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  // `useChannelHealth` disables itself when channelId is empty, so collapsed
  // rows don't poll. Each expanded channel fetches its own health on 10s poll.
  const { data: health } = useChannelHealth(expanded ? ch.id : '');
  const endpoints = ch.endpoints || [];

  return (
    <Fragment>
      <tr className="border-b last:border-0 hover:bg-muted/50">
        {/* Expand toggle */}
        <td className="py-3 pl-2 pr-1 w-8">
          <button
            type="button"
            onClick={() => setExpanded((v) => !v)}
            className="p-1 rounded hover:bg-muted-foreground/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-expanded={expanded}
            aria-label={expanded ? t('common.collapse') : t('common.expand')}
          >
            <ChevronRight
              className={cn('size-4 transition-transform', expanded && 'rotate-90')}
            />
          </button>
        </td>
        <td className="py-3 px-2 font-mono text-xs">{ch.id}</td>
        <td className="py-3 px-2">{ch.name || ch.id}</td>
        <td className="py-3 px-2">
          {PROVIDER_DISPLAY[ch.provider] || ch.provider}
          {ch.anthropic_compat && (
            <span className="ml-1.5 px-1.5 py-0.5 rounded text-[10px] bg-chart-4/15 text-chart-4 dark:bg-chart-4/40 dark:text-chart-4">
              Anthropic
            </span>
          )}
        </td>
        <td className="py-3 px-2 text-center">{ch.priority}</td>
        <td className="py-3 px-2 text-center">{endpoints.length}</td>
        <td className="py-3 px-2 text-center">
          <Switch
            checked={ch.enabled}
            onCheckedChange={() => toggleEnabled(ch)}
            disabled={togglePending}
          />
        </td>
        <td className="py-3 px-2 text-right whitespace-nowrap">
          <Button variant="ghost" size="sm" onClick={() => onEdit(ch)}>
            <Pencil className="size-3.5" />
          </Button>
          <Button variant="ghost" size="sm" onClick={() => onDelete(ch)}>
            <Trash2 className="size-3.5 text-destructive" />
          </Button>
        </td>
      </tr>

      {/* Expanded endpoint detail row */}
      {expanded && (
        <tr className="border-b bg-muted/20">
          <td colSpan={8} className="p-0">
            <div className="py-3 pr-4 pl-12">
              {endpoints.length === 0 ? (
                <div className="text-sm text-muted-foreground py-2">{t('endpoint.noEndpoints')}</div>
              ) : (
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b text-muted-foreground">
                      <th className="text-left py-2 pr-3 font-medium">{t('endpoint.id')}</th>
                      <th className="text-left py-2 pr-3 font-medium">{t('endpoint.url')}</th>
                      <th className="text-center py-2 pr-3 font-medium w-16">{t('endpoint.weight')}</th>
                      <th className="text-center py-2 pr-3 font-medium w-24">{t('endpoint.timeoutSecs')}</th>
                      <th className="text-center py-2 font-medium w-28">{t('endpoint.status')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {endpoints.map((ep) => {
                      const status = getEndpointStatus(ep, health?.endpoints);
                      return (
                        <tr key={ep.id ?? ep.url} className="border-b last:border-0 hover:bg-muted/30">
                          <td className="py-2 pr-3 font-mono text-muted-foreground">
                            {ep.id ?? '—'}
                          </td>
                          <td className="py-2 pr-3 max-w-64 truncate" title={ep.url}>
                            {ep.url}
                          </td>
                          <td className="py-2 pr-3 text-center">{ep.weight}</td>
                          <td className="py-2 pr-3 text-center">{ep.timeout_secs ?? '—'}</td>
                          <td className="py-2 text-center">
                            <StatusBadge status={status} />
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </td>
        </tr>
      )}
    </Fragment>
  );
}

// ── Page ──

export default function Channels() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const { data: channels, isLoading, isError, refetch } = useChannels();
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
          ) : isError ? (
            <div className="flex items-center justify-center p-8">
              <div className="text-center">
                <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
                <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
              </div>
            </div>
          ) : channels && channels.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="w-8 px-2 py-3" />
                    <th className="text-left py-3 px-2">{t('table.id')}</th>
                    <th className="text-left py-3 px-2">{t('table.name')}</th>
                    <th className="text-left py-3 px-2">{t('table.provider')}</th>
                    <th className="text-center py-3 px-2">{t('table.priority')}</th>
                    <th className="text-center py-3 px-2">{t('table.endpoints')}</th>
                    <th className="text-center py-3 px-2">{t('table.statusLabel')}</th>
                    <th className="text-right py-3 px-2">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {channels.map((ch) => (
                    <ChannelRow
                      key={ch.id}
                      ch={ch}
                      onEdit={setEditChannel}
                      onDelete={setDeleteTarget}
                      toggleEnabled={(c) => toggleEnabled.mutate(c)}
                      togglePending={toggleEnabled.isPending}
                    />
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
          onSubmit={(data) => {
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