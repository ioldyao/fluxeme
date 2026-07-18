import { useState, useEffect, useMemo } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useModels, useCreateModel, useUpdateModel, useDeleteModel, usePublishModel, useModelHealthCheck } from '@/api/models';
import { useChannels } from '@/api/channels';
import { ModelForm } from '@/forms/ModelForm';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Label } from '@/components/ui/label';
import { Pencil, Trash2, Plus, RefreshCw, Activity, Import, Loader2, Search, GanttChartSquare } from 'lucide-react';
import { toast } from 'sonner';
import { cn } from '@/lib/utils';
import { api } from '@/api/client';
import { CURRENCY_SYMBOL, usePricingCurrency, useCurrency } from '@/store/currency';
import type { Model, UpstreamModel } from '@/types';

const CATEGORY_ORDER = ['chat', 'reasoning', 'tools', 'web', 'vision', 'rerank', 'embedding'];
const CATEGORY_LABELS: Record<string, string> = {
  chat: '对话', reasoning: '推理', tools: '工具', web: '网页', vision: '视觉', rerank: '重排序', embedding: '嵌入',
};

type SortKey = 'id' | 'name' | 'match' | 'channel' | 'ctx' | 'price' | 'status';

export default function Models() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useModels();
  const { data: channels } = useChannels();
  const channelName = (id: string) => channels?.find((c) => c.id === id)?.name || id;
  const createModel = useCreateModel();
  const deleteModel = useDeleteModel();
  const publishModel = usePublishModel();
  const modelHealthCheck = useModelHealthCheck();
  const { currency } = useCurrency();
  const { effectiveCurrency: getEffectiveCurrency } = usePricingCurrency();

  const [editModel, setEditModel] = useState<Model | null>(null);
  const updateModel = useUpdateModel(editModel?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Model | null>(null);
  const [hcLoading, setHcLoading] = useState(false);
  const [hcResults, setHcResults] = useState<Record<string, { channel_id: string; success: boolean; latency_ms: number }[]>>({});

  // Filters & sort
  const [search, setSearch] = useState('');
  const [modalFilter, setModalFilter] = useState('all');
  const [statusFilter, setStatusFilter] = useState('all');
  const [sortKey, setSortKey] = useState<SortKey>('id');
  const [sortDir, setSortDir] = useState(1);

  const runHealthCheck = async (modelId: string) => {
    try {
      const res = await modelHealthCheck.mutateAsync(modelId);
      setHcResults((prev) => ({ ...prev, [modelId]: res.channel_results.map((r) => ({ channel_id: r.channel_id, success: r.success, latency_ms: r.latency_ms })) }));
    } catch (e: any) {
      toast.error(e.message);
    }
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
      refetch();
      if (!models) return;
      let done = 0;
      const total = models.length;
      for (const m of models) {
        try {
          const hcRes = await modelHealthCheck.mutateAsync(m.id);
          setHcResults((prev) => ({ ...prev, [m.id]: hcRes.channel_results.map((r) => ({ channel_id: r.channel_id, success: r.success, latency_ms: r.latency_ms })) }));
        } catch { /* skip */ }
        done++;
      }
      toast.success(`Health check: ${res.channels_checked} channels, ${done}/${total} models probed`);
    } catch (e: any) {
      toast.error(e.message);
    } finally {
      setHcLoading(false);
    }
  };

  // ── Filter & sort logic ──
  const filteredModels = useMemo(() => {
    let rows = models ?? [];

    const q = search.toLowerCase().trim();
    if (q) {
      rows = rows.filter((m) =>
        m.id.toLowerCase().includes(q) ||
        m.name.toLowerCase().includes(q) ||
        m.channels.some((b) => channelName(b.channel_id).toLowerCase().includes(q))
      );
    }

    if (modalFilter !== 'all') {
      rows = rows.filter((m) => {
        const cats = m.category?.split(',').filter(Boolean) ?? [];
        return cats.includes(modalFilter);
      });
    }

    if (statusFilter !== 'all') {
      rows = rows.filter((m) =>
        statusFilter === 'published' ? m.published : !m.published
      );
    }

    rows.sort((a, b) => {
      let av: any, bv: any;
      switch (sortKey) {
        case 'id': av = a.id; bv = b.id; break;
        case 'name': av = a.name; bv = b.name; break;
        case 'match': av = a.model_pattern; bv = b.model_pattern; break;
        case 'channel': {
          const aHc = hcResults[a.id]?.[0];
          const bHc = hcResults[b.id]?.[0];
          av = aHc?.latency_ms ?? 99999;
          bv = bHc?.latency_ms ?? 99999;
          break;
        }
        case 'ctx': av = a.context_length ?? 0; bv = b.context_length ?? 0; break;
        case 'price': av = a.pricing.prompt_price; bv = b.pricing.prompt_price; break;
        case 'status': av = a.published ? 1 : 0; bv = b.published ? 1 : 0; break;
        default: av = a.id; bv = b.id;
      }
      if (typeof av === 'string') return av.localeCompare(bv) * sortDir;
      return (av - bv) * sortDir;
    });

    return rows;
  }, [models, search, modalFilter, statusFilter, sortKey, sortDir, hcResults, channels]);

  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir((d) => d * -1);
    } else {
      setSortKey(key);
      setSortDir(1);
    }
  };

  const SortArrow = ({ k }: { k: SortKey }) => (
    <span className={cn('inline-block ml-1.5 text-[10px] opacity-30 transition-all', sortKey === k && 'opacity-100 text-[#5EEAD4]')}>
      {sortKey === k ? (sortDir === 1 ? '▲' : '▼') : '▲'}
    </span>
  );

  const channelHc = (modelId: string, chId: string) => hcResults[modelId]?.find((r) => r.channel_id === chId);

  // Stats
  const totalPublished = models?.filter((m) => m.published).length ?? 0;
  const totalAlerts = models?.filter((m) => {
    const results = hcResults[m.id];
    return results?.some((r) => !r.success);
  }).length ?? 0;

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
      setSyncChannelId(''); setUpstreamModels([]); setSelectedIds(new Set());
      setFetching(false); setAdding(false); setFetched(false);
    }
  }, [syncOpen]);

  const handleFetch = async () => {
    if (!syncChannelId) return;
    setFetching(true);
    try {
      const ms = await api<UpstreamModel[]>(`/channels/${encodeURIComponent(syncChannelId)}/upstream-models`, { method: 'GET' });
      setUpstreamModels(ms); setSelectedIds(new Set()); setFetched(true);
    } catch (e: any) { toast.error(e.message); }
    finally { setFetching(false); }
  };

  const toggleSelect = (id: string) => setSelectedIds((p) => {
    const n = new Set(p); if (n.has(id)) n.delete(id); else n.add(id); return n;
  });
  const toggleSelectAll = () => setSelectedIds((p) => p.size === upstreamModels.length ? new Set() : new Set(upstreamModels.map((m) => m.id)));

  const handleAddSelected = async () => {
    if (selectedIds.size === 0) return;
    setAdding(true);
    const results = await Promise.allSettled(
      Array.from(selectedIds).map(async (modelId) => {
        const up = upstreamModels.find((m) => m.id === modelId);
        await api('/models', { method: 'POST', body: { id: modelId, name: modelId, model_pattern: modelId, pricing: { prompt_price: 0, completion_price: 0 }, channels: [{ channel_id: syncChannelId, priority: 0 }], context_length: up?.max_model_len ?? null, published: false } });
      })
    );
    const failures = results.filter((r) => r.status === 'rejected');
    qc.invalidateQueries({ queryKey: ['models'] });
    setAdding(false);
    toast.success(failures.length > 0
      ? t('model.addPartialSuccess', { success: results.length - failures.length, failures: failures.length })
      : t('model.addSuccess', { count: results.length }));
    setSyncOpen(false);
  };

  return (
    <div className="max-w-[1400px] mx-auto px-8 py-10 space-y-6" style={{ fontFamily: "'Inter', sans-serif" }}>
      {/* ── Top Bar ── */}
      <div className="flex items-end justify-between flex-wrap gap-5 mb-2">
        <div>
          <div className="flex items-center gap-2 text-xs font-mono tracking-wider text-[#5EEAD4] uppercase mb-2">
            <span className="w-1.5 h-1.5 rounded-full bg-[#5EEAD4] shadow-[0_0_8px_#5EEAD4] animate-pulse" />
            实时监控中
          </div>
          <h1 className="text-[28px] font-bold tracking-tight m-0">模型控制台</h1>
          <p className="text-sm text-[#8B98A5] mt-1.5">管理已接入的模型、渠道绑定与发布状态</p>
        </div>
        <div className="flex gap-6">
          <div className="text-right">
            <div className="font-mono text-[22px] font-semibold">{models?.length ?? '—'}</div>
            <div className="text-[11px] text-[#5C6773] uppercase tracking-wider mt-0.5">模型总数</div>
          </div>
          <div className="text-right">
            <div className="font-mono text-[22px] font-semibold text-[#5EEAD4]">{totalPublished}</div>
            <div className="text-[11px] text-[#5C6773] uppercase tracking-wider mt-0.5">已发布</div>
          </div>
          <div className="text-right">
            <div className={cn('font-mono text-[22px] font-semibold', totalAlerts > 0 ? 'text-[#F0B429]' : 'text-[#5C6773]')}>{totalAlerts}</div>
            <div className="text-[11px] text-[#5C6773] uppercase tracking-wider mt-0.5">渠道告警</div>
          </div>
        </div>
      </div>

      {/* ── Controls ── */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="relative flex-1 min-w-[220px]">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-[#5C6773] pointer-events-none" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="搜索 ID、名称或渠道…"
            className="w-full h-[42px] bg-[#161B22] border border-[#232B36] rounded-lg pl-10 pr-3.5 text-sm text-[#E6EDF3] placeholder-[#5C6773] font-sans outline-none transition-all focus:border-[#2DD4BF] focus:shadow-[0_0_0_3px_rgba(94,234,212,0.08)]"
          />
        </div>

        <div className="flex gap-2 flex-wrap">
          {['all', 'chat', 'reasoning', 'tools'].map((m) => (
            <button
              key={m}
              onClick={() => setModalFilter(m)}
              className={cn(
                'px-3.5 py-2 text-sm font-medium rounded-full border transition-all whitespace-nowrap',
                modalFilter === m
                  ? 'bg-[rgba(94,234,212,0.08)] border-[#2DD4BF] text-[#5EEAD4]'
                  : 'bg-[#161B22] border-[#232B36] text-[#8B98A5] hover:border-[#5C6773] hover:text-[#E6EDF3]'
              )}
            >
              {m === 'all' ? '全部模态' : CATEGORY_LABELS[m] || m}
            </button>
          ))}
        </div>

        <div className="flex gap-2 flex-wrap">
          {[{ v: 'all', l: '全部状态' }, { v: 'published', l: '已发布' }, { v: 'draft', l: '未发布' }].map((s) => (
            <button
              key={s.v}
              onClick={() => setStatusFilter(s.v)}
              className={cn(
                'px-3.5 py-2 text-sm font-medium rounded-full border transition-all whitespace-nowrap',
                statusFilter === s.v
                  ? 'bg-[rgba(94,234,212,0.08)] border-[#2DD4BF] text-[#5EEAD4]'
                  : 'bg-[#161B22] border-[#232B36] text-[#8B98A5] hover:border-[#5C6773] hover:text-[#E6EDF3]'
              )}
            >
              {s.l}
            </button>
          ))}
        </div>

        <button onClick={() => setShowAdd(true)} className="btn-primary inline-flex items-center gap-1.5 bg-[#5EEAD4] text-[#0A1210] font-semibold text-sm px-[18px] py-[11px] rounded-lg border-none cursor-pointer transition-all hover:shadow-[0_0_0_4px_rgba(94,234,212,0.08)] active:scale-[0.97]">
          <Plus className="w-3.5 h-3.5" />新增模型
        </button>
      </div>

      {/* ── Table ── */}
      <div className="bg-[#161B22] border border-[#232B36] rounded-2xl overflow-hidden">
        {isLoading ? (
          <div className="p-12 text-center text-sm text-[#8B98A5]">加载中...</div>
        ) : isError ? (
          <div className="p-12 text-center">
            <p className="text-sm text-[#F87171] mb-3">加载失败</p>
            <button onClick={() => refetch()} className="btn-primary inline-flex items-center gap-1.5 bg-[#5EEAD4] text-[#0A1210] font-semibold text-sm px-4 py-2 rounded-lg border-none cursor-pointer">重试</button>
          </div>
        ) : filteredModels.length === 0 ? (
          <div className="p-16 text-center text-[#5C6773]">
            <Search className="w-10 h-10 mx-auto mb-3 opacity-50" />
            <div className="text-sm">没有找到匹配的模型，换个关键词或筛选条件试试</div>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm border-collapse" style={{ fontFamily: "'Inter', sans-serif" }}>
              <thead>
                <tr className="bg-[#1B2129]">
                  {([{ k: 'id', l: 'ID' }, { k: 'name', l: '名称' }, { k: 'match', l: '模型匹配' }, { k: 'channel', l: '绑定渠道' }] as const).map(({ k, l }) => (
                    <th key={k} onClick={() => handleSort(k)} className="text-left text-[11px] font-semibold uppercase tracking-wider text-[#5C6773] px-[18px] py-4 border-b border-[#232B36] whitespace-nowrap cursor-pointer select-none hover:text-[#8B98A5]">
                      {l}<SortArrow k={k} />
                    </th>
                  ))}
                  <th className="text-left text-[11px] font-semibold uppercase tracking-wider text-[#5C6773] px-[18px] py-4 border-b border-[#232B36] cursor-default">模态类型</th>
                  {([{ k: 'ctx', l: '上下文' }, { k: 'price', l: '定价' }, { k: 'status', l: '发布' }] as const).map(({ k, l }) => (
                    <th key={k} onClick={() => handleSort(k)} className="text-left text-[11px] font-semibold uppercase tracking-wider text-[#5C6773] px-[18px] py-4 border-b border-[#232B36] whitespace-nowrap cursor-pointer select-none hover:text-[#8B98A5]">
                      {l}<SortArrow k={k} />
                    </th>
                  ))}
                  <th className="text-right text-[11px] font-semibold uppercase tracking-wider text-[#5C6773] px-[18px] py-4 border-b border-[#232B36]">操作</th>
                </tr>
              </thead>
              <tbody>
                {filteredModels.map((m) => (
                  <tr key={m.id} className="border-b border-[#1D242E] last:border-none hover:bg-[rgba(94,234,212,0.03)] transition-colors">
                    <td className="px-[18px] py-4"><span className="font-mono text-[13px] font-medium text-[#5EEAD4]">{m.id}</span></td>
                    <td className="px-[18px] py-4"><span className="font-semibold text-[#E6EDF3]">{m.name}</span></td>
                    <td className="px-[18px] py-4"><span className="font-mono text-[13px] text-[#8B98A5]">{m.model_pattern}</span></td>
                    <td className="px-[18px] py-4">
                      {m.channels.length > 0 ? (
                        <div className="flex flex-col gap-1.5">
                          {m.channels.map((b) => {
                            const hc = channelHc(m.id, b.channel_id);
                            const ok = hc?.success;
                            const lat = hc?.latency_ms;
                            return (
                              <div key={b.channel_id} className="flex items-center gap-2">
                                <span className={cn(
                                  'w-2 h-2 rounded-full relative flex-shrink-0',
                                  hc ? (ok ? 'bg-[#5EEAD4] shadow-[0_0_6px_#5EEAD4]' : 'bg-[#F87171] shadow-[0_0_6px_#F87171]') : 'bg-[#5C6773]'
                                )} />
                                <span className="text-[13px] text-[#E6EDF3]">{channelName(b.channel_id)}</span>
                                {lat != null && (
                                  <span className={cn('font-mono text-xs', lat > 5000 ? 'text-[#F87171] font-semibold' : 'text-[#5C6773]')}>
                                    {lat}ms
                                  </span>
                                )}
                              </div>
                            );
                          })}
                        </div>
                      ) : <span className="text-[#5C6773]">—</span>}
                    </td>
                    <td className="px-[18px] py-4">
                      <div className="flex gap-1.5 flex-wrap max-w-[180px]">
                        {(m.category?.split(',').filter(Boolean).sort((a, b) => CATEGORY_ORDER.indexOf(a) - CATEGORY_ORDER.indexOf(b)) ?? []).map((cat) => (
                          <span key={cat} className="text-[11px] px-[9px] py-[4px] rounded-md bg-[#1B2129] border border-[#232B36] text-[#8B98A5] whitespace-nowrap">
                            {CATEGORY_LABELS[cat] || cat}
                          </span>
                        ))}
                        {!m.category && <span className="text-[#5C6773]">—</span>}
                      </div>
                    </td>
                    <td className="px-[18px] py-4"><span className="font-mono text-[13px] text-[#E6EDF3]">{formatCtx(m.context_length)}</span></td>
                    <td className="px-[18px] py-4">
                      <div className="text-[13px] leading-relaxed">
                        {(() => {
                          const sym = CURRENCY_SYMBOL[getEffectiveCurrency(currency, m.id)];
                          const pp = m.pricing.prompt_price;
                          const cp = m.pricing.completion_price;
                          const crp = m.pricing.cache_read_price;
                          const fmt = (v: number) => v < 0.01 ? v.toFixed(4) : (Number.isInteger(v) ? v.toFixed(0) : v.toFixed(2));
                          return (
                            <>
                              输入 <span className="font-mono font-medium text-[#E6EDF3]">{sym}{fmt(pp)}</span><span className="text-[#5C6773] mx-0.5">/</span>输出 <span className="font-mono font-medium text-[#E6EDF3]">{sym}{fmt(cp)}</span>
                              {crp > 0 && <span className="block text-[12px] text-[#5C6773] mt-0.5">缓存 {sym}{fmt(crp)}</span>}
                            </>
                          );
                        })()}
                      </div>
                    </td>
                    <td className="px-[18px] py-4">
                      <span className={cn(
                        'inline-flex items-center gap-1.5 text-xs font-semibold px-[11px] py-[5px] rounded-full',
                        m.published
                          ? 'bg-[rgba(94,234,212,0.08)] text-[#5EEAD4] border border-[rgba(94,234,212,0.25)]'
                          : 'bg-[rgba(139,152,165,0.08)] text-[#8B98A5] border border-[#232B36]'
                      )}>
                        <span className="w-1.5 h-1.5 rounded-full bg-current" />
                        {m.published ? '已发布' : '未发布'}
                      </span>
                    </td>
                    <td className="px-[18px] py-4 text-right">
                      <div className="flex items-center justify-end gap-1.5">
                        <button
                          onClick={() => runHealthCheck(m.id)}
                          disabled={modelHealthCheck.isPending}
                          className="w-[30px] h-[30px] flex items-center justify-center rounded-lg border border-transparent bg-transparent text-[#5C6773] cursor-pointer transition-all hover:bg-[#1B2129] hover:border-[#232B36] hover:text-[#E6EDF3]"
                          title="健康检测"
                        >
                          <ChartNoAxesGantt className={cn('w-[15px] h-[15px]', modelHealthCheck.isPending && 'animate-pulse')} />
                        </button>
                        <button
                          onClick={() => setEditModel(m)}
                          className="w-[30px] h-[30px] flex items-center justify-center rounded-lg border border-transparent bg-transparent text-[#5C6773] cursor-pointer transition-all hover:bg-[#1B2129] hover:border-[#232B36] hover:text-[#E6EDF3]"
                          title="编辑"
                        >
                          <Pencil className="w-[15px] h-[15px]" />
                        </button>
                        <button
                          onClick={() => setDeleteTarget(m)}
                          className="w-[30px] h-[30px] flex items-center justify-center rounded-lg border border-transparent bg-transparent text-[#5C6773] cursor-pointer transition-all hover:bg-[rgba(248,113,113,0.08)] hover:border-[rgba(248,113,113,0.25)] hover:text-[#F87171]"
                          title="删除"
                        >
                          <Trash2 className="w-[15px] h-[15px]" />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {/* Bottom bar */}
        {!isLoading && !isError && models && models.length > 0 && (
          <div className="flex items-center gap-3 px-[18px] py-3 border-t border-[#232B36] bg-[#1B2129]">
            <button onClick={() => setSyncOpen(true)} className="inline-flex items-center gap-1.5 text-xs font-medium px-3 py-2 rounded-lg border border-[#232B36] bg-transparent text-[#8B98A5] cursor-pointer transition-all hover:border-[#5C6773] hover:text-[#E6EDF3]">
              <Import className="w-3.5 h-3.5" />{t('model.syncUpstream')}
            </button>
            <button onClick={handleHealthCheck} disabled={hcLoading} className="inline-flex items-center gap-1.5 text-xs font-medium px-3 py-2 rounded-lg border border-[#232B36] bg-transparent text-[#8B98A5] cursor-pointer transition-all hover:border-[#5C6773] hover:text-[#E6EDF3] disabled:opacity-50">
              <Activity className={cn('w-3.5 h-3.5', hcLoading && 'animate-pulse')} />{t('model.healthCheck')}
            </button>
            <button onClick={() => refetch()} className="inline-flex items-center gap-1.5 text-xs font-medium px-3 py-2 rounded-lg border border-[#232B36] bg-transparent text-[#8B98A5] cursor-pointer transition-all hover:border-[#5C6773] hover:text-[#E6EDF3]">
              <RefreshCw className="w-3.5 h-3.5" />{t('common.refresh')}
            </button>
          </div>
        )}
      </div>

      {/* ── Dialogs ── */}
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
                <Select value={syncChannelId} onValueChange={(v) => { setSyncChannelId(v ?? ''); setFetched(false); setUpstreamModels([]); setSelectedIds(new Set()); }}>
                  <SelectTrigger className="w-full"><SelectValue placeholder={t('model.selectChannelPlaceholder')} /></SelectTrigger>
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
                  <Checkbox checked={upstreamModels.length > 0 && selectedIds.size === upstreamModels.length} onCheckedChange={toggleSelectAll} />
                  {t('model.selectAll', { count: upstreamModels.length })}
                  <span className="ml-auto text-xs text-muted-foreground">{selectedIds.size}/{upstreamModels.length}</span>
                </label>
                <div className="max-h-72 overflow-y-auto border rounded-lg divide-y">
                  {upstreamModels.length === 0 ? (
                    <div className="py-10 text-center text-sm text-muted-foreground">{t('model.noUpstreamModels')}</div>
                  ) : (
                    upstreamModels.map((m) => (
                      <label key={m.id} className="flex items-center gap-3 px-3 py-2.5 hover:bg-muted/50 cursor-pointer select-none transition-colors">
                        <Checkbox checked={selectedIds.has(m.id)} onCheckedChange={() => toggleSelect(m.id)} />
                        <span className="flex-1 text-sm truncate">{m.id}</span>
                        {m.max_model_len != null && (
                          <span className="text-xs text-muted-foreground shrink-0">
                            {m.max_model_len >= 1_000_000 ? `${(m.max_model_len / 1_000_000).toFixed(0)}M` : `${(m.max_model_len / 1_000).toFixed(0)}K`}
                          </span>
                        )}
                      </label>
                    ))
                  )}
                </div>
                <div className="flex justify-end">
                  <Button onClick={handleAddSelected} disabled={selectedIds.size === 0 || adding}>
                    {adding ? <><Loader2 className="size-4 mr-1 animate-spin" />{t('model.adding')}</> : t('model.addSelected', { count: selectedIds.size })}
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
