import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useCurrency, CURRENCY_SYMBOL, type CurrencyCode } from '@/store/currency';
import { useAuth } from '@/store/auth';
import { usePermission, Guard } from '@/permissions';
import { useUpdateTimezone } from '@/api/auth';
import { PageHeader } from '@/components/PageHeader';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { api } from '@/api/client';
import type { GatewayRuntimeConfig } from '@/types';

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

const DEFAULT_GATEWAY_CONFIG: GatewayRuntimeConfig = {
  connect_timeout_secs: 10,
  unary_base_timeout_secs: 60,
  body_size_extra_secs_per_100kb: 5,
  stream_first_byte_timeout_secs: 60,
  stream_idle_timeout_secs: 30,
  stream_total_timeout_secs: 600,
  max_retries: 2,
  handler_timeout_secs: 240,
  cache_ttl_secs: 300,
  billing_enabled: false,
};

export default function SettingsPage() {
  const { t } = useTranslation();
  const { currency, rate, setCurrency, setRate } = useCurrency();
  const { timezone, setTimezone } = useAuth();
  const updateTimezone = useUpdateTimezone();
  const [allowPrivateIps, setAllowPrivateIps] = useState(true);
  const [loading, setLoading] = useState(true);
  const [gatewayConfig, setGatewayConfig] = useState<GatewayRuntimeConfig>(DEFAULT_GATEWAY_CONFIG);
  const [gatewayLoading, setGatewayLoading] = useState(true);
  const [gatewaySaving, setGatewaySaving] = useState(false);

  const isAdmin = usePermission('admin:settings');

  useEffect(() => {
    if (!isAdmin) {
      setLoading(false);
      return;
    }
    api<{ enabled: boolean }>('/settings/allow-private-ips')
      .then((r) => setAllowPrivateIps(r.enabled))
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [isAdmin]);

  useEffect(() => {
    if (!isAdmin) {
      setGatewayLoading(false);
      return;
    }
    api<GatewayRuntimeConfig>('/gateway/config')
      .then(setGatewayConfig)
      .catch(() => {})
      .finally(() => setGatewayLoading(false));
  }, [isAdmin]);

  const toggleAllowPrivateIps = async (checked: boolean) => {
    setAllowPrivateIps(checked);
    try {
      await api('/settings/allow-private-ips', {
        method: 'PUT',
        body: { enabled: checked },
      });
    } catch {
      setAllowPrivateIps(!checked);
    }
  };

  const toggleBilling = async (checked: boolean) => {
    const updated = { ...gatewayConfig, billing_enabled: checked };
    setGatewayConfig(updated);
    try {
      await api('/gateway/config', { method: 'PUT', body: updated });
    } catch {
      setGatewayConfig((prev) => ({ ...prev, billing_enabled: !checked }));
      toast.error('Failed to save billing configuration');
    }
  };

  const handleTimezoneChange = (tz: string) => {
    setTimezone(tz);
    updateTimezone.mutate(tz);
  };

  const updateGw = (key: keyof GatewayRuntimeConfig, value: string) => {
    const num = parseInt(value, 10);
    if (!isNaN(num) && num >= 0) {
      setGatewayConfig((prev) => ({ ...prev, [key]: num }));
    }
  };

  const saveGatewayConfig = async () => {
    setGatewaySaving(true);
    try {
      await api('/gateway/config', {
        method: 'PUT',
        body: gatewayConfig,
      });
      toast.success(t('settings.gatewaySaved'));
    } catch {
      toast.error('Failed to save gateway configuration');
    } finally {
      setGatewaySaving(false);
    }
  };

  const gw = (key: keyof GatewayRuntimeConfig) => gatewayConfig[key];

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

      <Guard perm="admin:settings">
        <Card>
          <CardContent className="p-6 space-y-6">
            <h2 className="text-sm font-semibold text-foreground mb-4">{t('settings.security')}</h2>
            <div className="flex items-start justify-between gap-4">
              <div className="flex-1 min-w-0">
                <Label className="text-sm">{t('settings.allowPrivateIps')}</Label>
                <p className="text-xs text-muted-foreground mt-0.5">{t('settings.allowPrivateIpsHint')}</p>
              </div>
              <Switch
                checked={allowPrivateIps}
                onCheckedChange={toggleAllowPrivateIps}
                disabled={loading}
              />
            </div>
          </CardContent>
        </Card>
      </Guard>

      <Guard perm="admin:gateway">
        <Card>
          <CardContent className="p-6 space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold text-foreground">{t('settings.timeouts')}</h2>
              <Button size="sm" onClick={saveGatewayConfig} disabled={gatewayLoading || gatewaySaving}>
                {gatewaySaving ? 'Saving...' : t('common.save')}
              </Button>
            </div>
            <div className="grid grid-cols-2 gap-x-6 gap-y-5">
              <TimeoutField
                label={t('settings.connectTimeout')}
                hint={t('settings.connectTimeoutHint')}
                value={gw('connect_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('connect_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.unaryTimeout')}
                hint={t('settings.unaryTimeoutHint')}
                value={gw('unary_base_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('unary_base_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.bodyExtra')}
                hint={t('settings.bodyExtraHint')}
                value={gw('body_size_extra_secs_per_100kb')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('body_size_extra_secs_per_100kb', v)}
              />
              <TimeoutField
                label={t('settings.streamFirstByte')}
                hint={t('settings.streamFirstByteHint')}
                value={gw('stream_first_byte_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('stream_first_byte_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.streamIdle')}
                hint={t('settings.streamIdleHint')}
                value={gw('stream_idle_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('stream_idle_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.streamTotal')}
                hint={t('settings.streamTotalHint')}
                value={gw('stream_total_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('stream_total_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.maxRetries')}
                hint={t('settings.maxRetriesHint')}
                value={gw('max_retries')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('max_retries', v)}
              />
              <TimeoutField
                label={t('settings.handlerTimeout')}
                hint={t('settings.handlerTimeoutHint')}
                value={gw('handler_timeout_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('handler_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.cacheTtl')}
                hint={t('settings.cacheTtlHint')}
                value={gw('cache_ttl_secs')}
                disabled={gatewayLoading}
                onChange={(v) => updateGw('cache_ttl_secs', v)}
              />
            </div>
          </CardContent>
        </Card>
      </Guard>

      <Guard perm="admin:gateway">
        <Card>
          <CardContent className="p-6 space-y-6">
            <h2 className="text-sm font-semibold text-foreground">{t('settings.billing')}</h2>
            <div className="flex items-start justify-between gap-4">
              <div className="flex-1 min-w-0">
                <Label className="text-sm">{t('settings.billingToggle')}</Label>
                <p className="text-xs text-muted-foreground mt-0.5">{t('settings.billingToggleHint')}</p>
              </div>
              <Switch
                checked={gatewayConfig.billing_enabled}
                onCheckedChange={toggleBilling}
                disabled={gatewayLoading}
              />
            </div>
          </CardContent>
        </Card>
      </Guard>
    </div>
  );
}

function TimeoutField({
  label,
  hint,
  value,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  value: number | boolean;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  return (
    <div className="space-y-1.5">
      <Label className="text-xs">{label}</Label>
      <Input
        type="number"
        min="0"
        className="w-full h-8 text-xs"
        value={Number(value)}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value)}
      />
      <p className="text-[11px] text-muted-foreground leading-tight">{hint}</p>
    </div>
  );
}
