import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { api } from '@/api/client';
import { PageHeader } from '@/components/PageHeader';
import { Guard, usePermission } from '@/permissions';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import type { GatewayRuntimeConfig } from '@/types';

const PROBE_INTERVAL_MIN = 10;
const PROBE_INTERVAL_MAX = 3600;

export default function AdminSettings() {
  const { t } = useTranslation();
  const canGateway = usePermission('admin:gateway');
  const [intervalSecs, setIntervalSecs] = useState<number>(60);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [gatewayConfig, setGatewayConfig] = useState<GatewayRuntimeConfig | null>(null);
  const [billingSaving, setBillingSaving] = useState(false);
  const [gatewaySaving, setGatewaySaving] = useState(false);
  const [allowPrivateIps, setAllowPrivateIps] = useState(true);
  const [privateIpsLoading, setPrivateIpsLoading] = useState(true);

  useEffect(() => {
    api<{ interval_secs: number }>('/settings/probe-interval')
      .then((r) => setIntervalSecs(r.interval_secs))
      .catch(() => {})
      .finally(() => setLoading(false));
    api<{ enabled: boolean }>('/settings/allow-private-ips')
      .then((r) => setAllowPrivateIps(r.enabled))
      .catch(() => {})
      .finally(() => setPrivateIpsLoading(false));
  }, []);

  useEffect(() => {
    if (!canGateway) return;
    api<GatewayRuntimeConfig>('/gateway/config')
      .then(setGatewayConfig)
      .catch(() => {});
  }, [canGateway]);

  const toggleBilling = async (checked: boolean) => {
    if (!gatewayConfig) return;
    const updated = { ...gatewayConfig, billing_enabled: checked };
    setGatewayConfig(updated);
    setBillingSaving(true);
    try {
      await api('/gateway/config', { method: 'PUT', body: updated });
      toast.success(t('settings.gatewaySaved'));
    } catch {
      setGatewayConfig((prev) => (prev ? { ...prev, billing_enabled: !checked } : prev));
      toast.error('Failed to save billing configuration');
    } finally {
      setBillingSaving(false);
    }
  };

  const toggleAllowPrivateIps = async (checked: boolean) => {
    setAllowPrivateIps(checked);
    try {
      await api('/settings/allow-private-ips', { method: 'PUT', body: { enabled: checked } });
    } catch {
      setAllowPrivateIps(!checked);
    }
  };

  const updateGw = (key: keyof GatewayRuntimeConfig, value: string) => {
    const num = parseInt(value, 10);
    if (!isNaN(num) && num >= 0) {
      setGatewayConfig((prev) => (prev ? { ...prev, [key]: num } : prev));
    }
  };

  const saveGatewayConfig = async () => {
    if (!gatewayConfig) return;
    setGatewaySaving(true);
    try {
      await api('/gateway/config', { method: 'PUT', body: gatewayConfig });
      toast.success(t('settings.gatewaySaved'));
    } catch {
      toast.error('Failed to save gateway configuration');
    } finally {
      setGatewaySaving(false);
    }
  };

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

  return (
    <div className="max-w-2xl space-y-6 animate-fade-in">
      <PageHeader title={t('settings.title')} description={t('settings.adminSubtitle')} />

      <Guard perm="admin:gateway">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">{t('settings.billing')}</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="flex items-start justify-between gap-4">
              <div className="flex-1 min-w-0">
                <Label className="text-sm">{t('settings.billingToggle')}</Label>
                <p className="text-xs text-muted-foreground mt-0.5">{t('settings.billingToggleHint')}</p>
              </div>
              <Switch
                checked={gatewayConfig?.billing_enabled ?? false}
                onCheckedChange={toggleBilling}
                disabled={!gatewayConfig || billingSaving}
              />
            </div>
          </CardContent>
        </Card>
      </Guard>

      <Guard perm="admin:gateway">
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">{t('settings.timeouts')}</CardTitle>
              <Button size="sm" onClick={saveGatewayConfig} disabled={!gatewayConfig || gatewaySaving}>
                {gatewaySaving ? 'Saving...' : t('common.save')}
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 gap-x-6 gap-y-5">
              <TimeoutField
                label={t('settings.connectTimeout')}
                hint={t('settings.connectTimeoutHint')}
                value={gatewayConfig?.connect_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('connect_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.unaryTimeout')}
                hint={t('settings.unaryTimeoutHint')}
                value={gatewayConfig?.unary_base_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('unary_base_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.bodyExtra')}
                hint={t('settings.bodyExtraHint')}
                value={gatewayConfig?.body_size_extra_secs_per_100kb ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('body_size_extra_secs_per_100kb', v)}
              />
              <TimeoutField
                label={t('settings.streamFirstByte')}
                hint={t('settings.streamFirstByteHint')}
                value={gatewayConfig?.stream_first_byte_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('stream_first_byte_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.streamIdle')}
                hint={t('settings.streamIdleHint')}
                value={gatewayConfig?.stream_idle_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('stream_idle_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.streamTotal')}
                hint={t('settings.streamTotalHint')}
                value={gatewayConfig?.stream_total_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('stream_total_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.maxRetries')}
                hint={t('settings.maxRetriesHint')}
                value={gatewayConfig?.max_retries ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('max_retries', v)}
              />
              <TimeoutField
                label={t('settings.handlerTimeout')}
                hint={t('settings.handlerTimeoutHint')}
                value={gatewayConfig?.handler_timeout_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('handler_timeout_secs', v)}
              />
              <TimeoutField
                label={t('settings.cacheTtl')}
                hint={t('settings.cacheTtlHint')}
                value={gatewayConfig?.cache_ttl_secs ?? 0}
                disabled={!gatewayConfig}
                onChange={(v) => updateGw('cache_ttl_secs', v)}
              />
            </div>
          </CardContent>
        </Card>
      </Guard>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.security')}</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1 min-w-0">
              <Label className="text-sm">{t('settings.allowPrivateIps')}</Label>
              <p className="text-xs text-muted-foreground mt-0.5">{t('settings.allowPrivateIpsHint')}</p>
            </div>
            <Switch
              checked={allowPrivateIps}
              onCheckedChange={toggleAllowPrivateIps}
              disabled={privateIpsLoading}
            />
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
