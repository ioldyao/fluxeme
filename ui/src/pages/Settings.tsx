import { useTranslation } from 'react-i18next';
import { useCurrency, CURRENCY_SYMBOL, CURRENCY_CODE, type CurrencyCode } from '@/store/currency';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Settings as SettingsIcon } from 'lucide-react';

export default function SettingsPage() {
  const { t } = useTranslation();
  const { currency, rate, setCurrency, setRate } = useCurrency();

  return (
    <div className="max-w-2xl mx-auto space-y-6 animate-fade-in">
      <div>
        <h1 className="text-2xl font-semibold">{t('settings.title')}</h1>
        <p className="text-sm text-muted-foreground">{t('settings.subtitle')}</p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base flex items-center gap-2">
            <SettingsIcon className="h-4 w-4" />
            {t('settings.currency')}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label>{t('settings.currencyLabel')}</Label>
            <Select value={currency} onValueChange={(v) => setCurrency(v as CurrencyCode)}>
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="usd">{CURRENCY_SYMBOL.usd} USD</SelectItem>
                <SelectItem value="cny">{CURRENCY_SYMBOL.cny} CNY</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t('settings.currencyHint')}</p>
          </div>

          <div className="space-y-2">
            <Label>{t('settings.rateLabel')}</Label>
            <Input
              type="number"
              step="0.01"
              min="0"
              className="w-40"
              value={rate}
              onChange={(e) => {
                const v = parseFloat(e.target.value);
                if (!isNaN(v) && v > 0) setRate(v);
              }}
            />
            <p className="text-xs text-muted-foreground">
              {t('settings.rateHint')} 1 USD = {rate} CNY
            </p>
          </div>

          <div className="p-3 rounded-lg bg-muted/50">
            <p className="text-sm text-muted-foreground">{t('settings.preview')}</p>
            <p className="text-xl font-bold mt-1">
              {CURRENCY_SYMBOL[currency]}{(100 * (currency === 'cny' ? rate : 1)).toFixed(2)} {CURRENCY_CODE[currency]}
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
