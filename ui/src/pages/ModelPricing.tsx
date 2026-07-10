import { useState, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useModels, useUpdateModelPricing } from '@/api/models';
import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE } from '@/store/currency';
import type { Pricing } from '@/types';

const PRICE_FIELDS: { key: keyof Pricing; labelKey: string }[] = [
  { key: 'prompt_price', labelKey: 'pricing.inputPrice' },
  { key: 'completion_price', labelKey: 'pricing.outputPrice' },
  { key: 'cache_read_price', labelKey: 'pricing.cacheRead' },
  { key: 'cache_write_price', labelKey: 'pricing.cacheWrite' },
  { key: 'image_input_price', labelKey: 'pricing.image' },
  { key: 'audio_input_price', labelKey: 'pricing.audioInput' },
  { key: 'audio_output_price', labelKey: 'pricing.audioOutput' },
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

export default function ModelPricingPage() {
  const { t } = useTranslation();
  const { data: models, isLoading, isError, refetch } = useModels();
  const updatePricing = useUpdateModelPricing();
  const { currency, rate } = useCurrency();
  const sym = CURRENCY_SYMBOL[currency];
  const code = CURRENCY_CODE[currency];

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dirty, setDirty] = useState<Record<string, Pricing>>({});
  const [saving, setSaving] = useState<Record<string, boolean>>({});

  const selected = useMemo(
    () => models?.find((m) => m.id === selectedId) ?? null,
    [models, selectedId],
  );

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

  const previewPrices = currentValues;

  if (isLoading) {
    return (
      <div className="max-w-6xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />
        <Card><CardContent className="p-12 text-center text-sm text-muted-foreground">{t('common.loading')}</CardContent></Card>
      </div>
    );
  }

  if (isError) {
    return (
      <div className="max-w-6xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} actions={<Button variant="outline" size="sm" onClick={() => refetch()}>{t('common.refresh')}</Button>} />
        <Card><CardContent className="p-12 text-center text-sm text-destructive">{t('err.loadFailed')}</CardContent></Card>
      </div>
    );
  }

  if (!models || models.length === 0) {
    return (
      <div className="max-w-6xl mx-auto space-y-6 animate-fade-in">
        <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />
        <EmptyState message="No models configured." />
      </div>
    );
  }

  const selectedDirty = selectedId ? (dirty[selectedId] != null) : false;

  return (
    <div className="max-w-6xl mx-auto space-y-6 animate-fade-in">
      <PageHeader title={t('pricing.title')} description={t('pricing.subtitle')} />

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Model list */}
        <Card className="lg:col-span-1">
          <CardContent className="p-0">
            <div className="divide-y">
              {models.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => setSelectedId(m.id)}
                  className={`w-full text-left px-4 py-3 text-sm transition-colors hover:bg-muted/50 ${
                    selectedId === m.id ? 'bg-muted font-semibold' : ''
                  }`}
                >
                  <div className="truncate">{m.name || m.id}</div>
                  <div className="text-xs text-muted-foreground truncate mt-0.5">{m.id}</div>
                </button>
              ))}
            </div>
          </CardContent>
        </Card>

        {/* Pricing editor */}
        <Card className="lg:col-span-2">
          {!selected ? (
            <CardContent className="p-12 text-center text-sm text-muted-foreground">
              {t('pricing.selectModel')}
            </CardContent>
          ) : (
            <CardContent className="p-6 space-y-6">
              <div>
                <h2 className="text-lg font-semibold">{selected.name || selected.id}</h2>
                <p className="text-xs text-muted-foreground mt-0.5">{selected.id}</p>
              </div>

              <div className="space-y-4">
                {PRICE_FIELDS.map(({ key, labelKey }) => (
                  <div key={key} className="flex items-center justify-between gap-4">
                    <Label className="text-sm min-w-[8rem]">{t(labelKey)}</Label>
                    <div className="flex items-center gap-2">
                      <Input
                        type="number"
                        step="0.01"
                        min="0"
                        className="w-28 h-8 text-xs text-right"
                        value={currentValues?.[key] ?? 0}
                        onChange={(e) => {
                          const v = parseFloat(e.target.value);
                          setPrice(key, isNaN(v) ? 0 : Math.max(0, v));
                        }}
                      />
                      <span className="text-xs text-muted-foreground w-20">$/1M</span>
                    </div>
                  </div>
                ))}
              </div>

              <div className="flex gap-2 pt-2">
                <Button
                  size="sm"
                  disabled={!selectedDirty || saving[selected.id]}
                  onClick={() => handleSave(selected.id)}
                >
                  {saving[selected.id] ? (t('pricing.saving') || 'Saving...') : t('common.save')}
                </Button>
              </div>

              {/* Preview */}
              <div className="border-t pt-6">
                <p className="text-xs text-muted-foreground mb-3">{t('pricing.preview')}</p>
                <div className="rounded-xl border bg-card p-4 space-y-2">
                  {PRICE_FIELDS.map(({ key, labelKey }) => {
                    const v = previewPrices?.[key] ?? 0;
                    const label = t(labelKey);
                    return (
                      <div key={key} className="flex justify-between text-sm">
                        <span className="text-muted-foreground">{label}</span>
                        <span className="font-semibold">
                          {v > 0
                            ? t('pricing.perMillion', { price: `${sym}${(currency === 'cny' ? v * rate : v).toFixed(2)}` })
                            : t('pricing.empty')}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </CardContent>
          )}
        </Card>
      </div>
    </div>
  );
}
