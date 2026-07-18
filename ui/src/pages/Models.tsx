import { useState, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useModels, useCreateModel, useUpdateModel, useDeleteModel, usePublishModel, useModelHealthCheck } from '@/api/models';
import { useChannels } from '@/api/channels';
import { ModelForm } from '@/forms/ModelForm';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Pencil, Trash2, Plus, RefreshCw, Activity, Import, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { api } from '@/api/client';
import { CURRENCY_SYMBOL, usePricingCurrency, useCurrency } from '@/store/currency';
import type { Model, UpstreamModel } from '@/types';

const CATEGORY_ORDER = ['chat', 'reasoning', 'tools', 'web', 'vision', 'rerank', 'embedding'];

export default function Models() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useModels();
  const { data: channels } = useChannels();
  const channelName = (id: string) => channels?.find((c) => c.id === id)?.name || id;
  const createModel = useCreateModel();
  const deleteModel = useDeleteModel();
  const publishModel = usePublishModel();
  const [editModel, setEditModel] = useState<Model | null>(null);
  const updateModel = useUpdateModel(editModel?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Model | null>(null);
  const [hcLoading, setHcLoading] = useState(false);
  const { currency } = useCurrency();
  const { effectiveCurrency: getEffectiveCurrency } = usePricingCurrency();
  const modelHealthCheck = useModelHealthCheck();
  const [hcResults, setHcResults] = useState<Record<string, { channel_id: string; success: boolean; latency_ms: number }[]>>({});

  const runHealthCheck = async (modelId: string) => {
    try {
      const res = await modelHealthCheck.mutateAsync(modelId);
      setHcResults((prev) => ({ ...prev, [modelId]: res.channel_results.map((r) => ({ channel_id: r.channel_id, success: r.success, latency_ms: r.latency_ms })) }));
    } catch (e: any) {
      toast.error(e.message);
    }
  };

  const [allHcLoading, setAllHcLoading] = useState(false);
  const runAllHealthChecks = async () => {
    if (!models) return;
    setAllHcLoading(true);
    let done = 0;
    const total = models.length;
    for (const m of models) {
      try {
        const res = await modelHealthCheck.mutateAsync(m.id);
        setHcResults((prev) => ({ ...prev, [m.id]: res.channel_results.map((r) => ({ channel_id: r.channel_id, success: r.success, latency_ms: r.latency_ms })) }));
      } catch { /* skip */ }
      done++;
      toast.loading(`Health check: ${done}/${total}`, { id: 'all-hc' });
    }
    toast.success(`Health check complete: ${done}/${total}`, { id: 'all-hc' });
    setAllHcLoading(false);
  };

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteModel.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  const formatCtx = (v: number | null | undefined) => {
    if (!v) return '-';
    if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
    if (v >= 1_000) return `${(v / 1_000).toFixed(0)}K`;
    return v.toLocaleString();
  };

  const handleHealthCheck = async () => {
    setHcLoading(true);
    try {
      const res = await api<{ models_updated: number; channels_checked: number; channels_failed: number }>('/health-check/models', { method: 'POST' });
      if (res.channels_failed > 0) {
        toast.warning(t('model.healthCheckResultWithFailures', { channels: res.channels_checked, models: res.models_updated, failed: res.channels_failed }));
      } else {
        toast.success(t('model.healthCheckResult', { channels: res.channels_checked, models: res.models_updated }));
      }
      refetch();
    } catch (e: any) {
      toast.error(e.message);
    } finally {
      setHcLoading(false);
    }
  };

  // ── SyncUpstreamDialog ──
  const qc = useQueryClient();
  const [syncChannelId, setSyncChannelId] = useState('');
  const [upstreamModels, setUpstreamModels] = useState<UpstreamModel[]>([]);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [fetching, setFetching] = useState(false);
  const [adding, setAdding] = useState(false);
  const [fetched, setFetched] = useState(false);

  useEffect(() => {
    if (!syncOpen) {
      setSyncChannelId('');
      setUpstreamModels([]);
      setSelectedIds(new Set());
      setFetching(false);
      setAdding(false);
      setFetched(false);
    }
  }, [syncOpen]);

  const handleFetch = async () => {
    if (!syncChannelId) return;
    setFetching(true);
    try {
      const models = await api<UpstreamModel[]>(`/channels/${encodeURIComponent(syncChannelId)}/upstream-models`, { method: 'GET' });
      setUpstreamModels(models);
      setSelectedIds(new Set());
      setFetched(true);
    } catch (e: any) {
      toast.error(e.message);
    } finally {
      setFetching(false);
    }
  };

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const toggleSelectAll = () => {
    if (selectedIds.size === upstreamModels.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(upstreamModels.map((m) => m.id)));
    }
  };

  const handleAddSelected = async () => {
    if (selectedIds.size === 0) return;
    setAdding(true);
    const results = await Promise.allSettled(
      Array.from(selectedIds).map(async (modelId) => {
        const upstream = upstreamModels.find((m) => m.id === modelId);
        await api('/models', {
          method: 'POST',
          body: {
            id: modelId, name: modelId, model_pattern: modelId,
            pricing: { prompt_price: 0, completion_price: 0 },
            channels: [{ channel_id: syncChannelId, priority: 0 }],
            context_length: upstream?.max_model_len ?? null,
            published: false,
          },
        });
      })
    );
    const failures = results.filter(r => r.status === 'rejected');
    qc.invalidateQueries({ queryKey: ['models'] });
    setAdding(false);
    if (failures.length > 0) {
      toast.success(t('model.addPartialSuccess', { success: results.length - failures.length, failures: failures.length }));
    } else {
      toast.success(t('model.addSuccess', { count: results.length }));
    }
    setSyncOpen(false);
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('model.title')}
        description={t('model.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => setSyncOpen(true)}>
              <Import className="size-4 mr-1" />{t('model.syncUpstream')}
            </Button>
            <Button variant="outline" size="sm" onClick={runAllHealthChecks} disabled={allHcLoading}>
              <Activity className={cn('size-4 mr-1', allHcLoading && 'animate-pulse')} />Model Probe
            </Button>
            <Button variant="outline" size="sm" onClick={handleHealthCheck} disabled={hcLoading}>
              <Activity className={cn('size-4 mr-1', hcLoading && 'animate-pulse')} />{t('model.healthCheck')}
            </Button>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('model.add')}
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
          ) : models && models.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.id')}</th>
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.modelPattern')}</th>
                    <th className="text-right py-3 px-4">{t('table.bindings')}</th>
                    <th className="text-left py-3 px-4">{t('model.category')}</th>
                    <th className="text-right py-3 px-4">{t('model.context')}</th>
                    <th className="text-right py-3 px-4">{t('table.price')}</th>
                    <th className="text-center py-3 px-4">{t('model.publishCol')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {models.map((m) => (
                    <tr key={m.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{m.id}</td>
                      <td className="py-3 px-4">{m.name}</td>
                      <td className="py-3 px-4 text-xs text-muted-foreground font-mono">{m.model_pattern}</td>
                      <td className="py-3 px-4 text-right text-xs">
                        {m.channels.length > 0 ? (
                          <div className="flex flex-col gap-0.5 items-end">
                            {m.channels.map((b) => {
                              const hc = hcResults[m.id]?.find((r) => r.channel_id === b.channel_id);
                              return (
                                <span key={b.channel_id} className="whitespace-nowrap">
                                  {hc ? (hc.success ? '🟢' : '🔴') : '⚪'} {channelName(b.channel_id)}
                                  {hc ? ` ${hc.latency_ms}ms` : ''}
                                </span>
                              );
                            })}
                          </div>
                        ) : '-'}
                      </td>
                      <td className="py-3 px-4 text-xs">
                        {(() => {
                          const cats = m.category?.split(',').filter(Boolean).sort((a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b)) ?? [];
                          return cats.length > 0 ? (
                            <div className="flex flex-wrap gap-1">
                              {cats.map((cat) => (
                                <span key={cat} className="inline-block px-1.5 py-0.5 text-[10px] font-medium rounded bg-muted text-muted-foreground">
                                  {t(`model.category.${cat}`, { defaultValue: cat })}
                                </span>
                              ))}
                            </div>
                          ) : '-';
                        })()}
                      </td>
                      <td className="py-3 px-4 text-right text-xs font-mono">{formatCtx(m.context_length)}</td>
                      <td className="py-3 px-4 text-right text-xs">
                        {(() => {
                          const sym = CURRENCY_SYMBOL[getEffectiveCurrency(currency, m.id)];
                          const pp = m.pricing.prompt_price;
                          const cp = m.pricing.completion_price;
                          const crp = m.pricing.cache_read_price;
                          const fmt = (v: number) => Number.isInteger(v) ? v.toString() : parseFloat(v.toFixed(10)).toString();
                          const parts = [`${t('pricing.inputLabel')} ${sym}${fmt(pp)} / ${t('pricing.outputLabel')} ${sym}${fmt(cp)}`];
                          if (crp > 0) parts.push(`${t('pricing.cacheLabel')} ${sym}${fmt(crp)}`);
                          return parts.join(' · ');
                        })()}
                      </td>
                      <td className="py-3 px-4 text-center">
                        <Button
                          variant={m.published ? "outline" : "secondary"}
                          size="sm"
                          className={cn('h-7 text-xs', m.published ? 'text-green-600 border-green-300' : 'text-muted-foreground')}
                          onClick={() => publishModel.mutate(m.id, { onError: (err) => toast.error(err.message) })}
                          disabled={publishModel.isPending}
                        >
                          {m.published ? t('model.published') : t('model.publish')}
                        </Button>
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => runHealthCheck(m.id)} disabled={modelHealthCheck.isPending}>
                          <Activity className={cn('size-3.5', modelHealthCheck.isPending && 'animate-pulse')} />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setEditModel(m)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(m)}>
                          <Trash2 className="size-3.5 text-destructive" />
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
          onSubmit={(data: any) => {
            if (editModel) {
              updateModel.mutate(data, {
                onSuccess: () => { toast.success(t('toast.updated')); setEditModel(null); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            } else {
              createModel.mutate(data, {
                onSuccess: () => { toast.success(t('toast.created')); setShowAdd(false); refetch(); },
                onError: (err) => toast.error(err.message),
              });
            }
          }}
          isPending={createModel.isPending || updateModel.isPending}
        />
      )}
      <Dialog open={syncOpen} onOpenChange={setSyncOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t('model.syncTitle')}</DialogTitle>
            <p className="text-sm text-muted-foreground">{t('model.syncSubtitle')}</p>
          </DialogHeader>

          <div className="space-y-4">
            <div className="flex items-end gap-2">
              <div className="flex-1 space-y-1.5 min-w-0">
                <Label className="text-xs">{t('model.selectChannel')}</Label>
                <Select value={syncChannelId} onValueChange={(v) => {
                  setSyncChannelId(v ?? '');
                  setFetched(false); setUpstreamModels([]); setSelectedIds(new Set());
                }}>
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder={t('model.selectChannelPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {channels?.map((ch) => (
                      <SelectItem key={ch.id} value={ch.id} className="truncate">{ch.name || ch.id}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <Button onClick={handleFetch} disabled={!syncChannelId || fetching} className="shrink-0 h-9">
                {fetching && <Loader2 className="size-4 mr-1 animate-spin" />}
                {fetching ? t('model.fetching') : t('model.fetchModels')}
              </Button>
            </div>

            {fetched && (
              <div className="space-y-3">
                <label className="flex items-center gap-2 text-sm cursor-pointer select-none">
                  <Checkbox
                    checked={upstreamModels.length > 0 && selectedIds.size === upstreamModels.length}
                    onCheckedChange={toggleSelectAll}
                  />
                  {t('model.selectAll', { count: upstreamModels.length })}
                  <span className="ml-auto text-xs text-muted-foreground">
                    {selectedIds.size}/{upstreamModels.length}
                  </span>
                </label>

                <div className="max-h-72 overflow-y-auto border rounded-lg divide-y">
                  {upstreamModels.length === 0 ? (
                    <div className="py-10 text-center text-sm text-muted-foreground">{t('model.noUpstreamModels')}</div>
                  ) : (
                    upstreamModels.map((m) => (
                      <label
                        key={m.id}
                        className="flex items-center gap-3 px-3 py-2.5 hover:bg-muted/50 cursor-pointer select-none transition-colors"
                      >
                        <Checkbox
                          checked={selectedIds.has(m.id)}
                          onCheckedChange={() => toggleSelect(m.id)}
                        />
                        <span className="flex-1 text-sm truncate">{m.id}</span>
                        {m.max_model_len != null && (
                          <span className="text-xs text-muted-foreground shrink-0">
                            {m.max_model_len >= 1_000_000
                              ? `${(m.max_model_len / 1_000_000).toFixed(0)}M`
                              : `${(m.max_model_len / 1_000).toFixed(0)}K`}
                          </span>
                        )}
                      </label>
                    ))
                  )}
                </div>

                <div className="flex justify-end">
                  <Button onClick={handleAddSelected} disabled={selectedIds.size === 0 || adding}>
                    {adding ? (
                      <><Loader2 className="size-4 mr-1 animate-spin" />{t('model.adding')}</>
                    ) : (
                      t('model.addSelected', { count: selectedIds.size })
                    )}
                  </Button>
                </div>
              </div>
            )}
          </div>
        </DialogContent>
      </Dialog>
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
