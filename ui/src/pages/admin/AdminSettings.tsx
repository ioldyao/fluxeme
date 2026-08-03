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

  useEffect(() => {
    api<{ interval_secs: number }>('/settings/probe-interval')
      .then((r) => setIntervalSecs(r.interval_secs))
      .catch(() => {})
      .finally(() => setLoading(false));
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
