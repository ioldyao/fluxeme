import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { api } from '@fluxeme/shared/src/api/client';
import { fetchAppConfig, saveCurrencySettings } from '@fluxeme/shared/src/api/settings';
import { CURRENCY_SYMBOL, useCurrency, type CurrencyCode } from '@fluxeme/shared/src/store/currency';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';

const PROBE_INTERVAL_MIN = 10;
const PROBE_INTERVAL_MAX = 3600;

export default function AdminSettings() {
  const { t } = useTranslation();
  const { currency: globalCurrency, setCurrency: setGlobalCurrency } = useCurrency();
  const [localCurrency, setLocalCurrency] = useState<string>(globalCurrency);
  const [intervalSecs, setIntervalSecs] = useState<number>(60);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [currencySaving, setCurrencySaving] = useState(false);

  useEffect(() => {
    api<{ interval_secs: number }>('/settings/probe-interval')
      .then((r) => setIntervalSecs(r.interval_secs))
      .catch(() => {})
      .finally(() => setLoading(false));
    // Load currency settings into local state
    fetchAppConfig().then((r) => {
      setLocalCurrency(r.currency);
    }).catch(() => {});
  }, []);

  const save = async () => {
    const value = Math.round(intervalSecs);
    if (Number.isNaN(value) || value < PROBE_INTERVAL_MIN || value > PROBE_INTERVAL_MAX) {
      toast.error(
        t('settings.probeIntervalInvalid', {
          min: PROBE_INTERVAL_MIN,
          max: PROBE_INTERVAL_MAX,
        }),
      );
      return;
    }
    setSaving(true);
    try {
      const r = await api<{ interval_secs: number }>('/settings/probe-interval', {
        method: 'PUT',
        body: { interval_secs: value },
      });
      setIntervalSecs(r.interval_secs);
      toast.success(t('settings.probeIntervalSaved'));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('settings.saveFailed'));
    } finally {
      setSaving(false);
    }
  };

  const saveCurrency = async () => {
    if (!['usd', 'cny'].includes(localCurrency)) {
      toast.error('Invalid currency');
      return;
    }
    setCurrencySaving(true);
    try {
      const r = await saveCurrencySettings(localCurrency);
      setLocalCurrency(r.currency);
      setGlobalCurrency(r.currency as CurrencyCode);
      toast.success('Currency settings saved');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to save currency settings');
    } finally {
      setCurrencySaving(false);
    }
  };

  return (
    <div className="max-w-2xl space-y-6 animate-fade-in">
      <PageHeader title={t('settings.title')} description={t('settings.adminSubtitle')} />

      {/* ── Currency ──────────────────────────────────────────────── */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.currency')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1 min-w-0">
              <Label className="text-sm">{t('settings.currencyLabel')}</Label>
              <p className="text-xs text-muted-foreground mt-0.5">{t('settings.currencyHint')}</p>
            </div>
            <Select value={localCurrency} onValueChange={(v) => { setLocalCurrency(v); }}>
              <SelectTrigger className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="usd">{CURRENCY_SYMBOL.usd} USD</SelectItem>
                <SelectItem value="cny">{CURRENCY_SYMBOL.cny} CNY</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="flex justify-end">
            <Button onClick={saveCurrency} disabled={loading || currencySaving}>
              {t('common.save')}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.probeInterval')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-end gap-3 flex-wrap">
            <div className="flex-1 min-w-[220px]">
              <Label htmlFor="probe-interval" className="text-sm">
                {t('settings.probeIntervalLabel')}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5 mb-2">
                {t('settings.probeIntervalHint', {
                  min: PROBE_INTERVAL_MIN,
                  max: PROBE_INTERVAL_MAX,
                })}
              </p>
              <Input
                id="probe-interval"
                type="number"
                min={PROBE_INTERVAL_MIN}
                max={PROBE_INTERVAL_MAX}
                value={Number.isNaN(intervalSecs) ? '' : intervalSecs}
                onChange={(e) => setIntervalSecs(Number(e.target.value))}
                disabled={loading}
                className="max-w-[180px]"
              />
            </div>
            <Button onClick={save} disabled={loading || saving}>
              {t('common.save')}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground leading-relaxed">
            {t('settings.probeIntervalCostHint')}
          </p>
        </CardContent>
      </Card>
    </div>
  );
}
