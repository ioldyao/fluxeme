import { useTranslation } from 'react-i18next';
import { useCurrency, CURRENCY_SYMBOL, type CurrencyCode } from '@/store/currency';
import { useAuth } from '@/store/auth';
import { useUpdateTimezone, useUpdateCurrency } from '@/api/auth';
import { PageHeader } from '@/components/PageHeader';
import { Card, CardContent } from '@fluxeme/ui/card';
import { Input } from '@fluxeme/ui/input';
import { Label } from '@fluxeme/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/ui/select';

const COMMON_TIMEZONES: string[] = [
  'UTC',
  'Asia/Shanghai',
  'Asia/Hong_Kong',
  'Asia/Tokyo',
  'Asia/Seoul',
  'Asia/Singapore',
  'Asia/Taipei',
  'Asia/Bangkok',
  'Asia/Kolkata',
  'Asia/Dubai',
  'Europe/London',
  'Europe/Paris',
  'Europe/Berlin',
  'Europe/Moscow',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'America/Sao_Paulo',
  'Australia/Sydney',
  'Pacific/Auckland',
];

export default function SettingsPage() {
  const { t } = useTranslation();
  const { currency, rate, setCurrency, setRate } = useCurrency();
  const { timezone, setTimezone } = useAuth();
  const updateTimezone = useUpdateTimezone();
  const updateCurrency = useUpdateCurrency();

  const handleTimezoneChange = (nextTimezone: string) => {
    setTimezone(nextTimezone);
    updateTimezone.mutate(nextTimezone);
  };

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
                <Select
                  value={currency}
                  onValueChange={(value) => {
                    setCurrency(value as CurrencyCode);
                    updateCurrency.mutate(value);
                  }}
                >
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
                  <p className="text-xs text-muted-foreground mt-0.5">{t('settings.rateHint')}</p>
                </div>
                <div className="flex items-center gap-2">
                  <Input
                    type="number"
                    step="0.01"
                    min="0"
                    className="w-24"
                    value={rate}
                    onChange={(event) => {
                      const value = parseFloat(event.target.value);
                      if (!Number.isNaN(value) && value > 0) {
                        setRate(value);
                      }
                    }}
                  />
                  <span className="text-xs text-muted-foreground whitespace-nowrap">
                    1 USD = {rate} CNY
                  </span>
                </div>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardContent className="p-6 space-y-6">
          <h2 className="text-sm font-semibold text-foreground mb-4">{t('settings.timezone')}</h2>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1 min-w-0">
              <Label className="text-sm">{t('settings.timezoneLabel')}</Label>
            </div>
            <Select value={timezone} onValueChange={handleTimezoneChange}>
              <SelectTrigger className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {COMMON_TIMEZONES.map((tz) => (
                  <SelectItem key={tz} value={tz}>
                    {tz}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
