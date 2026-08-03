import { useTranslation } from 'react-i18next';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { useUpdateTimezone } from '@fluxeme/shared/src/api/auth';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';

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
  const { timezone, setTimezone } = useAuth();
  const updateTimezone = useUpdateTimezone();

  const handleTimezoneChange = (tz: string) => {
    setTimezone(tz);
    updateTimezone.mutate(tz);
  };

  return (
    <div className="max-w-2xl mx-auto space-y-6 animate-fade-in">
      <PageHeader title={t('settings.title')} description={t('settings.subtitle')} />

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
