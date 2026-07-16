import { useState, useMemo, useId } from 'react';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import { toast } from 'sonner';
import { useModels, useUpdateModelPricing } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useCurrency, CURRENCY_SYMBOL } from '@/store/currency';
import type { Pricing } from '@/types';

const PRICE_GROUPS: { label: string; fields: { key: keyof Pricing; labelKey: string }[] }[] = [
  {
    label: 'Core',
    fields: [
      { key: 'prompt_price', labelKey: 'pricing.inputPrice' },
      { key: 'completion_price', labelKey: 'pricing.outputPrice' },
    ],
  },
  {
    label: 'Cache',
    fields: [
      { key: 'cache_read_price', labelKey: 'pricing.cacheRead' },
      { key: 'cache_write_price', labelKey: 'pricing.cacheWrite' },
    ],
  },
  {
    label: 'Media',
    fields: [
      { key: 'image_input_price', labelKey: 'pricing.image' },
      { key: 'audio_input_price', labelKey: 'pricing.audioInput' },
      { key: 'audio_output_price', labelKey: 'pricing.audioOutput' },
    ],
  },
];

function toDisplay(p: Pricing): Pricing {
  return {
    prompt_price: +(p.prompt_price * 1000).toFixed(6),
    completion_price: +(p.completion_price * 1000).toFixed(6),
    cache_read_price: +(p.cache_read_price * 1000).toFixed(6),
    cache_write_price: +(p.cache_write_price * 1000).toFixed(6),
    image_input_price: +(p.image_input_price * 1000).toFixed(6),
    audio_input_price: +(p.audio_input_price * 1000).toFixed(6),
    audio_output_price: +(p.audio_output_price * 1000).toFixed(6),
  };
}

function toApi(p: Pricing): Pricing {
  return {
    prompt_price: +(p.prompt_price / 1000).toFixed(10),
    completion_price: +(p.completion_price / 1000).toFixed(10),
    cache_read_price: +(p.cache_read_price / 1000).toFixed(10),
    cache_write_price: +(p.cache_write_price / 1000).toFixed(10),
    image_input_price: +(p.image_input_price / 1000).toFixed(10),
    audio_input_price: +(p.audio_input_price / 1000).toFixed(10),
    audio_output_price: +(p.audio_output_price / 1000).toFixed(10),
  };
}

function PriceInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const uid = useId();
  return (
    <Input
      id={uid}
      type="number"
      step="0.01"
      min="0"
      className="w-24 h-8 text-xs text-right [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
      value={value}
      onChange={(e) => {
        const v = parseFloat(e.target.value);
        onChange(isNaN(v) ? 0 : Math.max(0, v));
      }}
    />
  );
}

export default function ModelPricingPage() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useModels();
  const updatePricing = useUpdateModelPricing();
  const { currency, rate } = useCurrency();

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dirty, setDirty] = useState<Record<string, Pricing>>({});
  const [saving, setSaving] = useState<Record<string, boolean>>({});
  const [searchQuery, setSearchQuery] = useState('');
  const [currencyMode, setCurrencyMode] = useState<'global' | 'per-model'>('global');
  const [modelCurrency, setModelCurrency] = useState<Record<string, 'usd' | 'cny'>>({});

  const filteredModels = useMemo(
    () => models?.filter((m) =>
      m.id.toLowerCase().includes(searchQuery.toLowerCase()) ||
      (m.name && m.name.toLowerCase().includes(searchQuery.toLowerCase()))
    ) ?? [],
    [models, searchQuery],
  );

  const selected = useMemo(
    () => models?.find((m) => m.id === selectedId) ?? null,
    [models, selectedId],
  );

  const effectiveCurrency = currencyMode === 'global'
    ? currency
    : (selectedId ? (modelCurrency[selectedId] ?? 'usd') : 'usd');

  const effectiveSym = CURRENCY_SYMBOL[effectiveCurrency];

  const currentValues = useMemo(
    () => (selected ? dirty[selected.id] ?? toDisplay(selected.pricing) : null),
    [selected, dirty],
  );

  function setPrice(field: keyof Pricing, value: number) {
    if (!selected) return;
    setDirty((prev) => ({
      ...prev,
      [selected.id]: { ...(prev[selected.id] ?? toDisplay(selected.pricing)), [field]: value },
    }));
  }

  async function handleSave(id: string) {
    const values = dirty[id] ?? (selected && toDisplay(selected.pricing));
    if (!values) return;
    setSaving((prev) => ({ ...prev, [id]: true }));
    try {
      await updatePricing.mutateAsync({ id, pricing: toApi(values) });
      setDirty((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
      toast.success(t('pricing.saved') || 'Pricing saved');
    } catch {
      toast.error(t('toast.failed'));
    } finally {
      setSaving((prev) => ({ ...prev, [id]: false }));
    }
  }

  if (isLoading) {
    return (
      <div className="max-w-4xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />
        <Card><CardContent className="p-12 text-center text-sm text-muted-foreground">{t('common.loading')}</CardContent></Card>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="max-w-4xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} actions={<Button variant="outline" size="sm" onClick={() => refetch()}>{t('common.refresh')}</Button>} />
        <Card><CardContent className="p-12 text-center text-sm text-destructive">{t('err.loadFailed')}</CardContent></Card>
      </div>
    );
  }

  if (!models || models.length === 0) {
    return (
      <div className="max-w-4xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />
        <EmptyState message="No models configured." />
      </div>
    );
  }

  return (
    <div className="max-w-4xl mx-auto space-y-6 animate-fade-in">
      <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />

      <Card>
        <CardContent className="p-5 sm:p-6 space-y-6">
          {/* Toolbar: search + model selector + mode + save */}
          <div className="space-y-3">
            <div className="flex items-center gap-2">
              <div className="relative flex-1 min-w-0">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground pointer-events-none" />
                <Input
                  placeholder={t('common.search') || 'Search...'}
                  className="pl-8 h-9 text-sm"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                />
              </div>
              <div className="flex rounded-lg border p-0.5 shrink-0">
                <button
                  type="button"
                  onClick={() => setCurrencyMode('global')}
                  className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                    currencyMode === 'global' ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
                  }`}
                >
                  {t('pricing.global') || 'Global'}
                </button>
                <button
                  type="button"
                  onClick={() => setCurrencyMode('per-model')}
                  className={`px-2.5 py-1 text-xs rounded-md font-medium transition-colors ${
                    currencyMode === 'per-model' ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
                  }`}
                >
                  {t('pricing.perModel') || 'Per'}
                </button>
              </div>
            </div>

            <div className="flex items-center gap-3">
              <div className="flex-1 min-w-0">
                <Select
                  value={selectedId ?? ''}
                  onValueChange={(v) => setSelectedId(v || null)}
                >
                  <SelectTrigger>
                    <SelectValue placeholder={t('pricing.selectModel')} />
                  </SelectTrigger>
                  <SelectContent>
                    {filteredModels.length === 0 ? (
                      <div className="py-6 text-center text-xs text-muted-foreground">{t('common.noResults') || 'No results'}</div>
                    ) : (
                      filteredModels.map((m) => (
                        <SelectItem key={m.id} value={m.id}>
                          <span className="truncate">{m.name || m.id}</span>
                          <span className="text-xs text-muted-foreground ml-2">{m.id}</span>
                        </SelectItem>
                      ))
                    )}
                  </SelectContent>
                </Select>
              </div>
              {selected && (
                <Button
                  size="sm"
                  disabled={!dirty[selected.id] || saving[selected.id]}
                  onClick={() => handleSave(selected.id)}
                  className="shrink-0"
                >
                  {saving[selected.id] ? (t('pricing.saving') || 'Saving...') : t('common.save')}
                </Button>
              )}
            </div>
          </div>

          {!selected ? (
            <div className="py-16 text-center text-sm text-muted-foreground select-none">
              {t('pricing.selectModel')}
            </div>
          ) : (
            <>
              {/* Selected model header */}
              <div className="flex items-center gap-3 pb-1">
                <div className="flex items-center justify-center size-10 rounded-full bg-brand/10 text-brand font-semibold text-sm shrink-0 select-none">
                  {(selected.name || selected.id).charAt(0).toUpperCase()}
                </div>
                <div className="min-w-0 flex-1">
                  <h2 className="text-sm font-semibold truncate">{selected.name || selected.id}</h2>
                  <p className="text-xs text-muted-foreground font-mono truncate">{selected.id}</p>
                </div>
                {currencyMode === 'per-model' && (
                  <div className="flex rounded-lg border p-0.5 shrink-0">
                    <button
                      type="button"
                      onClick={() => setModelCurrency((prev) => ({ ...prev, [selected.id]: 'usd' }))}
                      className={`px-2 py-0.5 text-[11px] rounded font-medium transition-colors ${
                        (modelCurrency[selected.id] ?? 'usd') === 'usd' ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
                      }`}
                    >
                      USD
                    </button>
                    <button
                      type="button"
                      onClick={() => setModelCurrency((prev) => ({ ...prev, [selected.id]: 'cny' }))}
                      className={`px-2 py-0.5 text-[11px] rounded font-medium transition-colors ${
                        modelCurrency[selected.id] === 'cny' ? 'bg-primary text-primary-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground'
                      }`}
                    >
                      CNY
                    </button>
                  </div>
                )}
              </div>

              {/* Price groups */}
              <div className="grid sm:grid-cols-3 gap-3">
                {PRICE_GROUPS.map((group) => (
                  <fieldset key={group.label} className="rounded-lg border p-3.5 space-y-3">
                    <legend className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider px-1">
                      {group.label}
                    </legend>
                    {group.fields.map(({ key, labelKey }) => (
                      <div key={key} className="flex items-center justify-between gap-2">
                        <Label className="text-xs text-muted-foreground shrink-0" htmlFor={`price-${key}`}>{t(labelKey)}</Label>
                        <PriceInput
                          value={currentValues?.[key] ?? 0}
                          onChange={(v) => setPrice(key, v)}
                        />
                      </div>
                    ))}
                  </fieldset>
                ))}
              </div>

              {/* Preview */}
              <div className="border-t pt-5">
                <p className="text-xs text-muted-foreground mb-3">{t('pricing.preview')}</p>
                <div className="rounded-lg bg-muted/40 p-4 space-y-1.5">
                  {PRICE_GROUPS.flatMap((g) => g.fields).map(({ key, labelKey }) => {
                    const v = currentValues?.[key] ?? 0;
                    return (
                      <div key={key} className="flex justify-between text-sm">
                        <span className="text-muted-foreground">{t(labelKey)}</span>
                        <span className={v > 0 ? 'font-medium tabular-nums' : 'text-muted-foreground'}>
                          {v > 0
                            ? t('pricing.perMillion', { price: `${effectiveSym}${(effectiveCurrency === 'cny' ? v * rate : v).toFixed(2)}` })
                            : t('pricing.empty')}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
