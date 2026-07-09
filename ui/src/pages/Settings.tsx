import { useTranslation } from 'react-i18next';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE, type CurrencyCode } from '@/store/currency';
import { PageHeader } from '@/components/PageHeader';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';

export default function SettingsPage() {
  const { t } = useTranslation();
  const { currency, rate, setCurrency, setRate } = useCurrency();

  return (
    <div className="max-w-2xl mx-auto space-y-6 animate-fade-in">
      <PageHeader title={t('settings.title')} description={t('settings.subtitle')} />

      <Card>
        <CardContent className="p-6 space-y-6">
          <div>
            <h2 className="text-sm font-semibold text-foreground mb-4">{t('settings.currency')}</h2>
            <div className="space-y-5">
              <div className="flex items-start justify-between gap-4">
                <div className="flex-1 min-w-0">
                  <Label className="text-sm">{t('settings.currencyLabel')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5">{t('settings.currencyHint')}</p>
                </div>
                <Select value={currency} onValueChange={(v) => setCurrency(v as CurrencyCode)}>
                  <SelectTrigger className="w-32">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="usd">{CURRENCY_SYMBOL.usd} USD</SelectItem>
                    <SelectItem value="cny">{CURRENCY_SYMBOL.cny} CNY</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div className="flex items-start justify-between gap-4">
                <div className="flex-1 min-w-0">
                  <Label className="text-sm">{t('settings.rateLabel')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    {t('settings.rateHint')}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="0.01"
                    min="0"
                    className="w-24"
                    value={rate}
                    onChange={(e) => {
                      const v = parseFloat(e.target.value);
                      if (!isNaN(v) && v > 0) setRate(v);
                    }}
                  />
                  <span className="text-xs text-muted-foreground whitespace-nowrap">
                    1 USD = {rate} CNY
                  </span>
                </div>
              </div>
            </div>
          </div>

          <div className="border-t pt-6">
            <p className="text-xs text-muted-foreground mb-2">{t('settings.preview')}</p>
            <div className="rounded-xl border bg-card p-4 flex items-center justify-between">
              <div>
                <p className="text-2xl font-bold tracking-tight">
                  {CURRENCY_SYMBOL[currency]}{(100 * (currency === 'cny' ? rate : 1)).toFixed(2)}
                </p>
                <p className="text-xs text-muted-foreground mt-0.5">
                  {CURRENCY_CODE[currency]}
                </p>
              </div>
              <div className="text-right">
                <p className="text-xs text-muted-foreground">{CURRENCY_CODE[currency]}</p>
                <p className="text-lg font-semibold text-brand">{CURRENCY_SYMBOL[currency]}{rate.toFixed(1)}</p>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
