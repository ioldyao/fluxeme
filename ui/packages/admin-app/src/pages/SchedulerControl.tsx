import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw, Save } from 'lucide-react';
import { toast } from 'sonner';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import {
  useSchedulerModelPolicy,
  useSchedulerModels,
  useSchedulerTopology,
  useSaveSchedulerModelPolicy,
  type SchedulerBindingTopology,
  type SchedulerModelPolicy,
} from '@fluxeme/shared/src/api/scheduler';

// ── Direction-aware model switch motion ────────────────────────────
// Clicking a model below the current one brings the next page in from below;
// clicking above reverses the direction. The transition is interruptible:
// any click cancels the pending timer and switches immediately, so rapid
// model changes never queue up or lock the list.
//
// No `filter: blur()` here — blurring a large subtree is what made the
// original animation feel janky. Cheap transforms + opacity only.

const PANEL_MOTION_CSS = `
@keyframes fxp-in-bottom { from { opacity: 0; transform: translateY(26px) scale(.985); } to { opacity: 1; transform: translateY(0) scale(1); } }
@keyframes fxp-in-top { from { opacity: 0; transform: translateY(-26px) scale(.985); } to { opacity: 1; transform: translateY(0) scale(1); } }
.fxp-in-up { animation: fxp-in-bottom .32s cubic-bezier(.16,1,.3,1) both; }
.fxp-in-down { animation: fxp-in-top .32s cubic-bezier(.16,1,.3,1) both; }
@media (prefers-reduced-motion: reduce) { .fxp-in-up, .fxp-in-down { animation: none !important; } }
`;

function clonePolicy(policy: SchedulerModelPolicy): SchedulerModelPolicy {
  return { ...policy, bindings: policy.bindings.map((b) => ({ ...b, endpoints: b.endpoints.map((e) => ({ ...e })) })) };
}

function numberOrNull(value: string): number | null {
  if (!value.trim()) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function pct(value: number | null | undefined, digits = 1): string {
  if (value == null || Number.isNaN(value)) return '—';
  return `${(value * 100).toFixed(digits)}%`;
}

// Status is a clean colored dot (no emoji). `title` keeps the meaning
// available for hover / screen readers.
const DOT_CLASS: Record<'healthy' | 'degraded' | 'open' | 'disabled', string> = {
  healthy: 'bg-emerald-500',
  degraded: 'bg-amber-500',
  open: 'bg-red-500',
  disabled: 'bg-muted-foreground/40',
};
function StatusDot({ state }: { state: 'healthy' | 'degraded' | 'open' | 'disabled' }) {
  return (
    <span
      className={`inline-block h-2.5 w-2.5 shrink-0 rounded-full ${DOT_CLASS[state]} shadow-[0_0_0_3px_rgba(0,0,0,0.05)]`}
      title={state}
      aria-label={state}
    />
  );
}

function endpointStatus(live?: SchedulerBindingTopology['endpoints'][number]): 'healthy' | 'open' | 'disabled' {
  if (!live) return 'disabled';
  if (!live.routing_available) return live.circuit_state === 'open' ? 'open' : 'disabled';
  return 'healthy';
}

function WeightCell({
  value,
  share,
  effective,
  onWeight,
}: {
  value: number;
  share: number | null;
  effective: number | null;
  onWeight: (v: string) => void;
}) {
  const width = share != null ? Math.min(100, share * 100) : 0;
  return (
    <div className="flex items-center gap-2 min-w-[170px]">
      <div className="h-2 flex-1 overflow-hidden rounded-full bg-muted">
        <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${width}%` }} />
      </div>
      <Input
        type="number"
        min={1}
        value={value}
        onChange={(e) => onWeight(e.target.value)}
        aria-label="weight"
        className="h-8 w-16 text-center text-xs tabular-nums"
      />
      <span className="w-14 shrink-0 text-right text-xs tabular-nums text-muted-foreground" title={effective != null && effective !== share ? `有效 ${pct(effective)}` : undefined}>
        {pct(share)}
      </span>
    </div>
  );
}

export default function SchedulerControl() {
  const { t } = useTranslation();
  const modelsQuery = useSchedulerModels();
  const [selectedModelId, setSelectedModelId] = useState('');
  const policyQuery = useSchedulerModelPolicy(selectedModelId);
  const topologyQuery = useSchedulerTopology(selectedModelId);
  const savePolicy = useSaveSchedulerModelPolicy(selectedModelId);
  const [draft, setDraft] = useState<SchedulerModelPolicy | null>(null);

  const [motion, setMotion] = useState('');
  const motionRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const modelIndexRef = useRef(-1);

  useEffect(() => {
    const list = modelsQuery.data ?? [];
    const first = list.find((m) => m.published) ?? list[0];
    if (!selectedModelId && first) {
      setSelectedModelId(first.id);
      modelIndexRef.current = list.indexOf(first);
    }
  }, [modelsQuery.data, selectedModelId]);

  useEffect(() => {
    if (policyQuery.data) setDraft(clonePolicy(policyQuery.data));
  }, [policyQuery.data]);

  useEffect(() => () => { if (motionRef.current) clearTimeout(motionRef.current); }, []);

  // Interruptible model switch: the panel div is keyed by `selectedModelId`,
  // so swapping the id remounts it and the enter animation replays. Any new
  // click cancels the pending clear timer and switches immediately — there is
  // no lockout window.
  const selectModel = (id: string, index: number) => {
    if (id === selectedModelId) return;
    const movingDown = index > modelIndexRef.current;
    modelIndexRef.current = index;
    if (motionRef.current) clearTimeout(motionRef.current);
    setSelectedModelId(id);
    setMotion(movingDown ? 'fxp-in-up' : 'fxp-in-down');
    motionRef.current = setTimeout(() => setMotion(''), 380);
  };

  const updateEndpoint = (bindingIndex: number, endpointIndex: number, field: 'weight' | 'timeout_secs' | 'max_tokens', value: string) => {
    setDraft((current) => (current ? {
      ...current,
      bindings: current.bindings.map((b, bi) => (bi !== bindingIndex ? b : {
        ...b,
        endpoints: b.endpoints.map((ep, ei) => (ei !== endpointIndex ? ep : {
          ...ep,
          [field]: field === 'weight' ? Math.max(1, Number(value) || 1) : numberOrNull(value),
        })),
      })),
    } : current));
  };

  const handleSave = () => {
    if (!draft) return;
    savePolicy.mutate(draft, {
      onSuccess: () => toast.success(t('scheduler.saved')),
      onError: (error) => toast.error(error.message),
    });
  };

  // Configured share denominators (from the editable draft, so bars update live).
  const modelConfiguredWeight = useMemo(
    () => (draft?.bindings ?? []).flatMap((b) => b.endpoints).reduce((sum, e) => sum + e.weight, 0),
    [draft],
  );
  const runtimeByChannel = useMemo(
    () => new Map((topologyQuery.data?.bindings ?? []).map((b) => [b.channel_id, b])),
    [topologyQuery.data],
  );

  return (
    <div className="space-y-4 animate-fade-in">
      <style>{PANEL_MOTION_CSS}</style>
      <PageHeader
        title={t('scheduler.title')}
        description={t('scheduler.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => { modelsQuery.refetch(); policyQuery.refetch(); topologyQuery.refetch(); }}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button size="sm" onClick={handleSave} disabled={!draft || savePolicy.isPending}>
              <Save className="size-4 mr-1" />{t('common.save')}
            </Button>
          </>
        }
      />

      <div className="grid gap-4 lg:grid-cols-[17rem_1fr]">
        <Card className="h-fit lg:sticky lg:top-4">
          <CardContent className="space-y-1 p-3">
            <div className="px-2 pb-1 text-xs font-semibold text-muted-foreground">{t('scheduler.models')}</div>
            {modelsQuery.isLoading ? <p className="px-2 py-2 text-sm text-muted-foreground">{t('common.loading')}</p> : null}
            {(modelsQuery.data ?? []).map((model, index) => (
              <button
                key={model.id}
                type="button"
                onClick={() => selectModel(model.id, index)}
                className={`w-full rounded-md px-3 py-2 text-left text-sm transition-colors ${selectedModelId === model.id ? 'bg-accent font-medium' : 'hover:bg-muted'}`}
              >
                <div className="truncate">{model.name || model.id}</div>
                <div className="mt-0.5 truncate font-mono text-[10px] text-muted-foreground">
                  {model.id} · {model.binding_count} {t('scheduler.bindings')}
                </div>
              </button>
            ))}
            {!modelsQuery.isLoading && !modelsQuery.data?.length ? (
              <p className="px-2 py-2 text-sm text-muted-foreground">{t('scheduler.noModels')}</p>
            ) : null}
          </CardContent>
        </Card>

        <div className={`min-w-0 ${motion}`} key={selectedModelId}>
          {policyQuery.isLoading || !draft ? (
            <Card><CardContent className="p-10 text-center text-muted-foreground">{t('common.loading')}</CardContent></Card>
          ) : (
            <div className="space-y-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                  <h2 className="text-lg font-semibold">{draft.model_name}</h2>
                  <p className="font-mono text-xs text-muted-foreground">{draft.model_id}</p>
                </div>
                <span className="rounded-full border px-3 py-1 text-xs text-muted-foreground">{t('scheduler.endpointCentric')}</span>
              </div>

              {/* Channel traffic overview — auto-computed from endpoint weights */}
              {draft.bindings.length > 0 && (
                <Card>
                  <CardContent className="p-4">
                    <div className="flex items-center justify-between gap-2 pb-3">
                      <span className="text-sm font-semibold">{t('scheduler.channelTraffic')}</span>
                      <span className="text-xs text-muted-foreground">{t('scheduler.autoWeight')}</span>
                    </div>
                    <div className="flex h-3 overflow-hidden rounded-full bg-muted">
                      {draft.bindings.map((b, i) => {
                        const total = b.endpoints.reduce((s, e) => s + e.weight, 0);
                        const width = modelConfiguredWeight > 0 ? (total / modelConfiguredWeight) * 100 : 0;
                        const live = runtimeByChannel.get(b.channel_id);
                        return <div key={b.channel_id} className="h-full transition-all" style={{ width: `${width}%`, background: `var(--chart-${(i % 5) + 1})` }} title={`${live?.channel_name || b.channel_id} ${pct(width / 100)}`} />;
                      })}
                    </div>
                    <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1 text-xs text-muted-foreground">
                      {draft.bindings.map((b, i) => {
                        const total = b.endpoints.reduce((s, e) => s + e.weight, 0);
                        const share = modelConfiguredWeight > 0 ? total / modelConfiguredWeight : 0;
                        const live = runtimeByChannel.get(b.channel_id);
                        return (
                          <span key={b.channel_id} className="flex items-center gap-2">
                            <i className="inline-block h-2 w-2 rounded-full" style={{ background: `var(--chart-${(i % 5) + 1})` }} />
                            {live?.channel_name || b.channel_id} {pct(share)}
                          </span>
                        );
                      })}
                    </div>
                  </CardContent>
                </Card>
              )}

              {draft.bindings.length === 0 ? (
                <Card><CardContent className="p-10 text-center text-muted-foreground">{t('scheduler.noBindings')}</CardContent></Card>
              ) : null}

              {draft.bindings.map((binding, bindingIndex) => {
                const live = runtimeByChannel.get(binding.channel_id);
                const channelConfigured = binding.endpoints.reduce((s, e) => s + e.weight, 0);
                const channelConfiguredShare = modelConfiguredWeight > 0 ? channelConfigured / modelConfiguredWeight : null;
                // Channel status: disabled ⚪, all-eligible 🟢, none 🔴, partial 🟡.
                let channelStatus: 'healthy' | 'degraded' | 'open' | 'disabled' = 'disabled';
                if (live) {
                  if (!live.channel_enabled) channelStatus = 'disabled';
                  else if (live.eligible_total_weight === 0) channelStatus = 'open';
                  else if (live.eligible_total_weight < live.configured_total_weight) channelStatus = 'degraded';
                  else channelStatus = 'healthy';
                }
                return (
                  <Card key={binding.channel_id}>
                    <CardContent className="p-4">
                      <div className="flex flex-wrap items-center justify-between gap-3 border-b pb-3">
                        <div className="flex min-w-0 items-center gap-2.5">
                          <StatusDot state={channelStatus} />
                          <div className="min-w-0">
                            <div className="truncate text-base font-semibold">{live?.channel_name || binding.channel_id}</div>
                            <div className="truncate font-mono text-[10px] text-muted-foreground">{binding.channel_id}{live?.provider ? ` · ${live.provider}` : ''}</div>
                          </div>
                        </div>
                        <div className="flex items-center gap-4 text-right text-xs text-muted-foreground">
                          <div>
                            <div className="text-base font-semibold tabular-nums text-foreground">{channelConfigured}</div>
                            <div>{t('scheduler.configuredWeight')}</div>
                          </div>
                          <div>
                            <div className="text-base font-semibold tabular-nums text-foreground">{pct(channelConfiguredShare)}</div>
                            <div>{t('scheduler.channelShare')}</div>
                          </div>
                          <div>
                            <div className="text-base font-semibold tabular-nums">{pct(live?.eligible_share ?? null)}</div>
                            <div>{t('scheduler.effectiveShare')}</div>
                          </div>
                        </div>
                      </div>

                      <div className="overflow-x-auto pt-2">
                        <table className="w-full min-w-[720px] text-xs">
                          <thead>
                            <tr className="border-b text-left text-muted-foreground">
                              <th className="py-2 pr-3 font-medium">{t('scheduler.endpoint')}</th>
                              <th className="w-56 py-2 pr-3 font-medium">{t('scheduler.weight')}</th>
                              <th className="w-36 py-2 pr-3 font-medium">{t('scheduler.timeout')}</th>
                              <th className="w-36 py-2 pr-3 font-medium">max_tokens</th>
                              <th className="w-12 py-2 text-center font-medium">{t('scheduler.status')}</th>
                            </tr>
                          </thead>
                          <tbody>
                            {binding.endpoints.length === 0 ? (
                              <tr><td colSpan={5} className="py-4 text-center text-muted-foreground">{t('scheduler.noEndpoints')}</td></tr>
                            ) : null}
                            {binding.endpoints.map((endpoint, endpointIndex) => {
                              const liveEp = live?.endpoints.find((ep) => ep.endpoint_id === endpoint.endpoint_id);
                              const share = modelConfiguredWeight > 0 ? endpoint.weight / modelConfiguredWeight : null;
                              const effectiveShare = liveEp ? (endpoint.weight / (topologyQuery.data?.eligible_total_weight || 1)) : null;
                              return (
                                <tr key={endpoint.endpoint_id} className="border-b last:border-0">
                                  <td className="max-w-[300px] py-2 pr-3">
                                    <div className="truncate font-mono" title={liveEp?.url}>{liveEp?.url || `#${endpoint.endpoint_id}`}</div>
                                    <div className="text-[10px] text-muted-foreground">ID {endpoint.endpoint_id}</div>
                                  </td>
                                  <td className="py-2 pr-3">
                                    <WeightCell
                                      value={endpoint.weight}
                                      share={share}
                                      effective={effectiveShare}
                                      onWeight={(v) => updateEndpoint(bindingIndex, endpointIndex, 'weight', v)}
                                    />
                                  </td>
                                  <td className="py-2 pr-3">
                                    <Input
                                      type="number"
                                      min={1}
                                      placeholder="—"
                                      value={endpoint.timeout_secs ?? ''}
                                      onChange={(e) => updateEndpoint(bindingIndex, endpointIndex, 'timeout_secs', e.target.value)}
                                      className="h-8 w-28 text-xs"
                                    />
                                  </td>
                                  <td className="py-2 pr-3">
                                    <Input
                                      type="number"
                                      min={1}
                                      placeholder={t('scheduler.unlimited')}
                                      value={endpoint.max_tokens ?? ''}
                                      onChange={(e) => updateEndpoint(bindingIndex, endpointIndex, 'max_tokens', e.target.value)}
                                      className="h-8 w-28 text-xs"
                                    />
                                  </td>
                                  <td className="py-2 text-center">
                                    <StatusDot state={endpointStatus(liveEp)} />
                                  </td>
                                </tr>
                              );
                            })}
                          </tbody>
                        </table>
                      </div>
                    </CardContent>
                  </Card>
                );
              })}

              <div className="flex flex-wrap gap-x-6 gap-y-1 border-t pt-3 text-xs text-muted-foreground">
                <span className="flex items-center gap-1.5"><StatusDot state="healthy" />{t('scheduler.statusHealthy')}</span>
                <span className="flex items-center gap-1.5"><StatusDot state="degraded" />{t('scheduler.statusDegraded')}</span>
                <span className="flex items-center gap-1.5"><StatusDot state="open" />{t('scheduler.statusOpen')}</span>
                <span className="flex items-center gap-1.5"><StatusDot state="disabled" />{t('scheduler.statusDisabled')}</span>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
