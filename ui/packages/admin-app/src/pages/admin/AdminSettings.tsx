import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { api } from '@fluxeme/shared/src/api/client';
import { usePublicModels } from '@fluxeme/shared/src/api/models';
import { useChannels } from '@fluxeme/shared/src/api/channels';
import {
  fetchAppConfig,
  saveCurrencySettings,
} from '@fluxeme/shared/src/api/settings';
import { CURRENCY_SYMBOL, useCurrency, type CurrencyCode } from '@fluxeme/shared/src/store/currency';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@fluxeme/shared/src/components/ui/tabs';
import { Guard, usePermission } from '@fluxeme/shared/src/permissions';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import SsoSettings from '@/pages/admin/SsoSettings';
import type { GatewayRuntimeConfig } from '@fluxeme/shared/src/types';

const PROBE_INTERVAL_MIN = 10;
const PROBE_INTERVAL_MAX = 3600;
const BREAKER_THRESHOLD_MIN = 1;
const BREAKER_THRESHOLD_MAX = 100;
const BREAKER_COOLDOWN_MIN = 0;
const BREAKER_COOLDOWN_MAX = 3600;
const BREAKER_LONG_FAIL_THRESHOLD_MIN = 1;
const BREAKER_LONG_FAIL_THRESHOLD_MAX = 1000;
const BREAKER_LONG_PROBE_INTERVAL_MIN = 60;
const BREAKER_LONG_PROBE_INTERVAL_MAX = 86400;

export default function AdminSettings() {
  const { t } = useTranslation();
  const { currency: globalCurrency, setCurrency: setGlobalCurrency } = useCurrency();
  const canGateway = usePermission('admin:gateway');
  const [localCurrency, setLocalCurrency] = useState<string>(globalCurrency);
  const [intervalSecs, setIntervalSecs] = useState<number>(60);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [currencySaving, setCurrencySaving] = useState(false);
  const [gatewayConfig, setGatewayConfig] = useState<GatewayRuntimeConfig | null>(null);
  const [billingSaving, setBillingSaving] = useState(false);
  const [gatewaySaving, setGatewaySaving] = useState(false);
  const [allowPrivateIps, setAllowPrivateIps] = useState(true);
  const [privateIpsLoading, setPrivateIpsLoading] = useState(true);
  const [breakerThreshold, setBreakerThreshold] = useState(3);
  const [breakerCooldown, setBreakerCooldown] = useState(30);
  const [breakerLongFailThreshold, setBreakerLongFailThreshold] = useState(10);
  const [breakerLongProbeInterval, setBreakerLongProbeInterval] = useState(1800);
  const [breakerLoading, setBreakerLoading] = useState(true);
  const [breakerSaving, setBreakerSaving] = useState(false);
  const [probePrompt, setProbePrompt] = useState('hi');
  const [probeMaxOutputTokens, setProbeMaxOutputTokens] = useState(1);
  const [probeTemperature, setProbeTemperature] = useState(0.01);
  const [probeTopP, setProbeTopP] = useState(0.01);
  const [probeTimeoutSecs, setProbeTimeoutSecs] = useState(30);
  const [probeProtocol, setProbeProtocol] = useState('auto');
  const [probePreviewTab, setProbePreviewTab] = useState('openai_chat');
  const [probeRequestLoading, setProbeRequestLoading] = useState(true);
  const [probeRequestSaving, setProbeRequestSaving] = useState(false);
  const [probePreviews, setProbePreviews] = useState<Record<string, string>>({});

  useEffect(() => {
    api<{ interval_secs: number }>('/settings/probe-interval')
      .then((r) => setIntervalSecs(r.interval_secs))
      .catch(() => {})
      .finally(() => setLoading(false));
    // Load currency settings into local state
    fetchAppConfig().then((r) => {
      setLocalCurrency(r.currency);
    }).catch(() => {});
    api<{ enabled: boolean }>('/settings/allow-private-ips')
      .then((r) => setAllowPrivateIps(r.enabled))
      .catch(() => {})
      .finally(() => setPrivateIpsLoading(false));
    api<{ threshold: number; cooldown_secs: number; long_fail_threshold?: number; long_probe_interval_secs?: number }>('/settings/breaker')
      .then((r) => {
        setBreakerThreshold(r.threshold);
        setBreakerCooldown(r.cooldown_secs);
        if (r.long_fail_threshold != null) setBreakerLongFailThreshold(r.long_fail_threshold);
        if (r.long_probe_interval_secs != null) setBreakerLongProbeInterval(r.long_probe_interval_secs);
      })
      .catch(() => {})
      .finally(() => setBreakerLoading(false));
    api<{ config: { prompt: string; max_output_tokens: number; temperature: number; top_p: number; timeout_secs: number; protocol: string }; previews: Record<string, string> }>('/settings/probe-request')
      .then((r) => {
        setProbePrompt(r.config.prompt);
        setProbeMaxOutputTokens(r.config.max_output_tokens);
        setProbeTemperature(r.config.temperature);
        setProbeTopP(r.config.top_p);
        setProbeTimeoutSecs(r.config.timeout_secs);
        setProbeProtocol(r.config.protocol);
        setProbePreviews(r.previews);
      })
      .catch(() => {})
      .finally(() => setProbeRequestLoading(false));
  }, []);

  useEffect(() => {
    if (!canGateway) return;
    api<GatewayRuntimeConfig>('/gateway/config')
      .then(setGatewayConfig)
      .catch(() => {});
  }, [canGateway]);

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

  const saveProbeRequest = async () => {
    const prompt = probePrompt.trim();
    const maxOutputTokens = Math.round(probeMaxOutputTokens);
    const temperature = Number(probeTemperature);
    const topP = Number(probeTopP);
    const timeoutSecs = Math.round(probeTimeoutSecs);
    if (
      !prompt ||
      Number.isNaN(maxOutputTokens) || maxOutputTokens < 1 || maxOutputTokens > 16 ||
      Number.isNaN(temperature) || temperature < 0 || temperature > 2 ||
      Number.isNaN(topP) || topP < 0 || topP > 1 ||
      Number.isNaN(timeoutSecs) || timeoutSecs < 1 || timeoutSecs > 120
    ) {
      toast.error(t('settings.probeIntervalInvalid', { min: 1, max: 120 }));
      return;
    }
    setProbeRequestSaving(true);
    try {
      const r = await api<{ config: { prompt: string; max_output_tokens: number; temperature: number; top_p: number; timeout_secs: number; protocol: string } }>('/settings/probe-request', {
        method: 'PUT',
        body: { prompt, max_output_tokens: maxOutputTokens, temperature, top_p: topP, timeout_secs: timeoutSecs, protocol: probeProtocol },
      });
      setProbePrompt(r.config.prompt);
      setProbeMaxOutputTokens(r.config.max_output_tokens);
      setProbeTemperature(r.config.temperature);
      setProbeTopP(r.config.top_p);
      setProbeTimeoutSecs(r.config.timeout_secs);
      setProbeProtocol(r.config.protocol);
      toast.success(t('settings.probeSaved'));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('settings.saveFailed'));
    } finally {
      setProbeRequestSaving(false);
    }
  };

  const saveBreaker = async () => {
    const threshold = Math.round(breakerThreshold);
    const cooldown = Math.round(breakerCooldown);
    const longFail = Math.round(breakerLongFailThreshold);
    const longProbe = Math.round(breakerLongProbeInterval);
    if (
      Number.isNaN(threshold) || Number.isNaN(cooldown) ||
      threshold < BREAKER_THRESHOLD_MIN || threshold > BREAKER_THRESHOLD_MAX ||
      cooldown < BREAKER_COOLDOWN_MIN || cooldown > BREAKER_COOLDOWN_MAX ||
      Number.isNaN(longFail) || longFail < BREAKER_LONG_FAIL_THRESHOLD_MIN || longFail > BREAKER_LONG_FAIL_THRESHOLD_MAX ||
      Number.isNaN(longProbe) || longProbe < BREAKER_LONG_PROBE_INTERVAL_MIN || longProbe > BREAKER_LONG_PROBE_INTERVAL_MAX
    ) {
      toast.error(
        t('settings.breakerInvalid', {
          thMin: BREAKER_THRESHOLD_MIN, thMax: BREAKER_THRESHOLD_MAX,
          cdMin: BREAKER_COOLDOWN_MIN, cdMax: BREAKER_COOLDOWN_MAX,
          lfMin: BREAKER_LONG_FAIL_THRESHOLD_MIN, lfMax: BREAKER_LONG_FAIL_THRESHOLD_MAX,
          lpMin: BREAKER_LONG_PROBE_INTERVAL_MIN, lpMax: BREAKER_LONG_PROBE_INTERVAL_MAX,
        }),
      );
      return;
    }
    setBreakerSaving(true);
    try {
      const r = await api<{ threshold: number; cooldown_secs: number; long_fail_threshold: number; long_probe_interval_secs: number }>('/settings/breaker', {
        method: 'PUT',
        body: { threshold, cooldown_secs: cooldown, long_fail_threshold: longFail, long_probe_interval_secs: longProbe },
      });
      setBreakerThreshold(r.threshold);
      setBreakerCooldown(r.cooldown_secs);
      setBreakerLongFailThreshold(r.long_fail_threshold);
      setBreakerLongProbeInterval(r.long_probe_interval_secs);
      toast.success(t('settings.breakerSaved'));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : t('settings.saveFailed'));
    } finally {
      setBreakerSaving(false);
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

  return (
    <div className="max-w-4xl space-y-6 animate-fade-in">
      <PageHeader title={t('settings.title')} description={t('settings.adminSubtitle')} />

      <Tabs defaultValue="gateway">
        <TabsList variant="line">
          <TabsTrigger value="gateway">{t('settings.gatewayTab')}</TabsTrigger>
          <TabsTrigger value="probe-request">{t('settings.probeRequestTab')}</TabsTrigger>
          <TabsTrigger value="sso">{t('ssoSettings.title')}</TabsTrigger>
        </TabsList>

        <TabsContent value="gateway" className="mt-6 space-y-6">
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

      {/* ── Security ─────────────────────────────────────────────── */}
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

      {/* ── Billing ───────────────────────────────────────────────── */}
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

      {/* ── Timeouts & Retries ────────────────────────────────────── */}
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

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.breakerParams')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-end gap-3 flex-wrap">
            <div className="flex-1 min-w-[220px]">
              <Label htmlFor="breaker-threshold" className="text-sm">
                {t('settings.breakerThreshold')}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5 mb-2">
                {t('settings.breakerThresholdHint', {
                  min: BREAKER_THRESHOLD_MIN,
                  max: BREAKER_THRESHOLD_MAX,
                })}
              </p>
              <Input
                id="breaker-threshold"
                type="number"
                min={BREAKER_THRESHOLD_MIN}
                max={BREAKER_THRESHOLD_MAX}
                value={Number.isNaN(breakerThreshold) ? '' : breakerThreshold}
                onChange={(e) => setBreakerThreshold(Number(e.target.value))}
                disabled={breakerLoading}
                className="max-w-[180px]"
              />
            </div>
            <div className="flex-1 min-w-[220px]">
              <Label htmlFor="breaker-cooldown" className="text-sm">
                {t('settings.breakerCooldown')}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5 mb-2">
                {t('settings.breakerCooldownHint', {
                  min: BREAKER_COOLDOWN_MIN,
                  max: BREAKER_COOLDOWN_MAX,
                })}
              </p>
              <Input
                id="breaker-cooldown"
                type="number"
                min={BREAKER_COOLDOWN_MIN}
                max={BREAKER_COOLDOWN_MAX}
                value={Number.isNaN(breakerCooldown) ? '' : breakerCooldown}
                onChange={(e) => setBreakerCooldown(Number(e.target.value))}
                disabled={breakerLoading}
                className="max-w-[180px]"
              />
            </div>
            <div className="flex-1 min-w-[220px]">
              <Label htmlFor="breaker-long-fail" className="text-sm">
                {t('settings.breakerLongFailThreshold')}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5 mb-2">
                {t('settings.breakerLongFailThresholdHint', {
                  min: BREAKER_LONG_FAIL_THRESHOLD_MIN,
                  max: BREAKER_LONG_FAIL_THRESHOLD_MAX,
                })}
              </p>
              <Input
                id="breaker-long-fail"
                type="number"
                min={BREAKER_LONG_FAIL_THRESHOLD_MIN}
                max={BREAKER_LONG_FAIL_THRESHOLD_MAX}
                value={Number.isNaN(breakerLongFailThreshold) ? '' : breakerLongFailThreshold}
                onChange={(e) => setBreakerLongFailThreshold(Number(e.target.value))}
                disabled={breakerLoading}
                className="max-w-[180px]"
              />
            </div>
            <div className="flex-1 min-w-[220px]">
              <Label htmlFor="breaker-long-probe" className="text-sm">
                {t('settings.breakerLongProbeInterval')}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5 mb-2">
                {t('settings.breakerLongProbeIntervalHint', {
                  min: BREAKER_LONG_PROBE_INTERVAL_MIN,
                  max: BREAKER_LONG_PROBE_INTERVAL_MAX,
                })}
              </p>
              <Input
                id="breaker-long-probe"
                type="number"
                min={BREAKER_LONG_PROBE_INTERVAL_MIN}
                max={BREAKER_LONG_PROBE_INTERVAL_MAX}
                value={Number.isNaN(breakerLongProbeInterval) ? '' : breakerLongProbeInterval}
                onChange={(e) => setBreakerLongProbeInterval(Number(e.target.value))}
                disabled={breakerLoading}
                className="max-w-[180px]"
              />
            </div>
            <Button onClick={saveBreaker} disabled={breakerLoading || breakerSaving}>
              {t('common.save')}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground leading-relaxed">
            {t('settings.breakerCostHint')}
          </p>
          <p className="text-xs text-muted-foreground leading-relaxed">
            {t('settings.breakerLongHint')}
          </p>
        </CardContent>
      </Card>
        </TabsContent>

        <TabsContent value="probe-request" className="mt-6 space-y-6">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">{t('settings.probeRequestTitle')}</CardTitle>
              <p className="text-sm text-muted-foreground">{t('settings.probeRequestSubtitle')}</p>
            </CardHeader>
            <CardContent className="space-y-5">
              <div className="grid gap-5 md:grid-cols-2">
                <div className="md:col-span-2">
                  <Label htmlFor="probe-prompt" className="text-sm">{t('settings.probePrompt')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probePromptHint')}</p>
                  <textarea
                    id="probe-prompt"
                    rows={3}
                    value={probePrompt}
                    onChange={(e) => setProbePrompt(e.target.value)}
                    disabled={probeRequestLoading}
                    className="w-full rounded-lg border border-border bg-muted px-3 py-2 text-sm outline-none placeholder:text-muted-foreground"
                  />
                </div>
                <div>
                  <Label htmlFor="probe-max-tokens" className="text-sm">{t('settings.probeMaxOutputTokens')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probeMaxOutputTokensHint')}</p>
                  <Input
                    id="probe-max-tokens"
                    type="number"
                    min={1}
                    max={16}
                    value={Number.isNaN(probeMaxOutputTokens) ? '' : probeMaxOutputTokens}
                    onChange={(e) => setProbeMaxOutputTokens(Number(e.target.value))}
                    disabled={probeRequestLoading}
                    className="max-w-[180px]"
                  />
                </div>
                <div>
                  <Label htmlFor="probe-timeout" className="text-sm">{t('settings.probeTimeout')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probeTimeoutHint')}</p>
                  <Input
                    id="probe-timeout"
                    type="number"
                    min={1}
                    max={120}
                    value={Number.isNaN(probeTimeoutSecs) ? '' : probeTimeoutSecs}
                    onChange={(e) => setProbeTimeoutSecs(Number(e.target.value))}
                    disabled={probeRequestLoading}
                    className="max-w-[180px]"
                  />
                </div>
                <div>
                  <Label htmlFor="probe-temperature" className="text-sm">{t('settings.probeTemperature')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probeTemperatureHint')}</p>
                  <Input
                    id="probe-temperature"
                    type="number"
                    step={0.01}
                    min={0}
                    max={2}
                    value={Number.isNaN(probeTemperature) ? '' : probeTemperature}
                    onChange={(e) => setProbeTemperature(Number(e.target.value))}
                    disabled={probeRequestLoading}
                    className="max-w-[180px]"
                  />
                </div>
                <div>
                  <Label htmlFor="probe-top-p" className="text-sm">{t('settings.probeTopP')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probeTopPHint')}</p>
                  <Input
                    id="probe-top-p"
                    type="number"
                    step={0.01}
                    min={0}
                    max={1}
                    value={Number.isNaN(probeTopP) ? '' : probeTopP}
                    onChange={(e) => setProbeTopP(Number(e.target.value))}
                    disabled={probeRequestLoading}
                    className="max-w-[180px]"
                  />
                </div>
                <div className="md:col-span-2">
                  <Label htmlFor="probe-protocol" className="text-sm">{t('settings.probeProtocol')}</Label>
                  <p className="text-xs text-muted-foreground mt-0.5 mb-2">{t('settings.probeProtocolHint')}</p>
                  <Select value={probeProtocol} onValueChange={setProbeProtocol} disabled={probeRequestLoading}>
                    <SelectTrigger id="probe-protocol" className="w-full max-w-[280px]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="auto">{t('settings.probeProtocolAuto')}</SelectItem>
                      <SelectItem value="openai_chat">{t('settings.probeProtocolOpenaiChat')}</SelectItem>
                      <SelectItem value="anthropic_messages">{t('settings.probeProtocolAnthropicMessages')}</SelectItem>
                      <SelectItem value="responses">{t('settings.probeProtocolResponses')}</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
              <div className="flex justify-end">
                <Button onClick={saveProbeRequest} disabled={probeRequestLoading || probeRequestSaving}>
                  {t('settings.probeSave')}
                </Button>
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-base">{t('settings.probePreview')}</CardTitle>
              <p className="text-sm text-muted-foreground">{t('settings.probePreviewHint')}</p>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="inline-flex rounded-xl bg-accent p-1">
                {['openai_chat', 'anthropic_messages', 'responses'].map((key) => (
                  <button
                    key={key}
                    type="button"
                    onClick={() => setProbePreviewTab(key)}
                    className={`rounded-lg px-3 py-1.5 text-xs transition ${
                      probePreviewTab === key
                        ? 'bg-card text-foreground shadow-sm'
                        : 'text-muted-foreground hover:text-foreground'
                    }`}
                  >
                    {key === 'openai_chat'
                      ? t('settings.probeProtocolOpenaiChat')
                      : key === 'anthropic_messages'
                        ? t('settings.probeProtocolAnthropicMessages')
                        : t('settings.probeProtocolResponses')}
                  </button>
                ))}
              </div>
              <pre className="overflow-x-auto rounded-xl border border-border bg-muted p-4 text-xs leading-relaxed">
                {probePreviews[probePreviewTab] ?? '—'}
              </pre>
            </CardContent>
          </Card>

          <ProbeTestRunner />
        </TabsContent>

        <TabsContent value="sso" className="mt-6">
          <SsoSettings />
        </TabsContent>
      </Tabs>
    </div>
  );
}

interface ProbeTestResult {
  success: boolean;
  model: string;
  channel_id: string;
  endpoint_id?: number | null;
  endpoint_url: string;
  upstream_model: string;
  protocol: string;
  latency_ms: number;
  ttft_ms?: number | null;
  error_kind?: string | null;
  error_message?: string | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
}

function ProbeTestRunner() {
  const { t } = useTranslation();
  const modelsQuery = usePublicModels();
  const channelsQuery = useChannels();
  const [modelId, setModelId] = useState('');
  const [channelId, setChannelId] = useState('');
  const [endpointId, setEndpointId] = useState('');
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<ProbeTestResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const model = modelsQuery.data?.find((m) => m.id === modelId);
  const modelChannelIds = new Set((model?.channels ?? []).map((c) => c.channel_id));
  const availableChannels = (channelsQuery.data ?? []).filter(
    (c) => !modelId || modelChannelIds.has(c.id),
  );
  const selectedChannel = availableChannels.find((c) => c.id === channelId);
  const availableEndpoints = (selectedChannel?.endpoints ?? []).filter(
    (e) => e.enabled !== false,
  );

  useEffect(() => {
    if (availableChannels.length > 0 && !availableChannels.some((c) => c.id === channelId)) {
      setChannelId(availableChannels[0].id);
      setEndpointId('');
    }
  }, [availableChannels, channelId]);

  useEffect(() => {
    if (availableEndpoints.length > 0 && !availableEndpoints.some((e) => String(e.id) === endpointId)) {
      setEndpointId(availableEndpoints[0].id != null ? String(availableEndpoints[0].id) : '');
    }
  }, [availableEndpoints, endpointId]);

  const send = async () => {
    if (!modelId || !channelId) return;
    setTesting(true);
    setResult(null);
    setError(null);
    try {
      const res = await api<ProbeTestResult>('/settings/probe-request/test', {
        method: 'POST',
        body: {
          model_id: modelId,
          channel_id: channelId,
          endpoint_id: endpointId ? Number(endpointId) : null,
        },
      });
      setResult(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : t('settings.saveFailed'));
    } finally {
      setTesting(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">{t('settings.probeTestTitle')}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <p className="text-sm text-muted-foreground">{t('settings.probeTestHint')}</p>
        <div className="grid gap-4 md:grid-cols-3">
          <div>
            <Label className="text-sm">{t('settings.probeTestModel')}</Label>
            <Select value={modelId} onValueChange={(v) => { setModelId(v); setEndpointId(''); }}>
              <SelectTrigger className="mt-1.5 w-full">
                <SelectValue placeholder={t('settings.probeTestSelectModel')} />
              </SelectTrigger>
              <SelectContent>
                {(modelsQuery.data ?? []).map((m) => (
                  <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <Label className="text-sm">{t('settings.probeTestChannel')}</Label>
            <Select value={channelId} onValueChange={(v) => { setChannelId(v); setEndpointId(''); }}>
              <SelectTrigger className="mt-1.5 w-full">
                <SelectValue placeholder={t('settings.probeTestSelectChannel')} />
              </SelectTrigger>
              <SelectContent>
                {availableChannels.map((c) => (
                  <SelectItem key={c.id} value={c.id}>{c.name || c.id}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div>
            <Label className="text-sm">{t('settings.probeTestEndpoint')}</Label>
            <Select value={endpointId} onValueChange={setEndpointId}>
              <SelectTrigger className="mt-1.5 w-full">
                <SelectValue placeholder={t('settings.probeTestSelectEndpoint')} />
              </SelectTrigger>
              <SelectContent>
                {availableEndpoints.map((e) => (
                  <SelectItem key={e.id ?? e.url} value={e.id != null ? String(e.id) : e.url}>
                    {e.id != null ? `#${e.id} · ${e.url}` : e.url}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="flex justify-end">
          <Button onClick={send} disabled={testing || !modelId || !channelId}>
            {testing ? t('settings.probeTestRunning') : t('settings.probeTestSend')}
          </Button>
        </div>

        {error ? (
          <div className="rounded-xl border border-destructive/20 bg-destructive/5 px-4 py-3 text-sm text-destructive">
            {error}
          </div>
        ) : null}

        {result ? (
          <div className="rounded-xl border border-border bg-muted p-4">
            <div className="flex items-center justify-between gap-3">
              <div className={`inline-flex items-center gap-2 text-sm font-semibold ${result.success ? 'text-chart-2' : 'text-destructive'}`}>
                <span className={`h-2.5 w-2.5 rounded-full ${result.success ? 'bg-chart-2' : 'bg-destructive'}`} />
                {result.success ? t('settings.probeTestSuccess') : t('settings.probeTestFailed')}
              </div>
              <div className="text-xs text-muted-foreground">
                {t('settings.probeTestProtocol')}: {result.protocol}
              </div>
            </div>
            <div className="mt-3 grid gap-2 text-xs sm:grid-cols-2">
              <div className="flex justify-between gap-3"><span className="text-muted-foreground">{t('settings.probeTestEndpointUrl')}</span><span className="max-w-[60%] truncate font-mono text-foreground">{result.endpoint_url}</span></div>
              <div className="flex justify-between gap-3"><span className="text-muted-foreground">{t('settings.probeTestLatency')}</span><span className="font-mono text-foreground">{result.latency_ms} ms</span></div>
              <div className="flex justify-between gap-3"><span className="text-muted-foreground">TTFT</span><span className="font-mono text-foreground">{result.ttft_ms == null ? '—' : `${result.ttft_ms} ms`}</span></div>
              <div className="flex justify-between gap-3"><span className="text-muted-foreground">Upstream</span><span className="font-mono text-foreground">{result.upstream_model}</span></div>
              <div className="flex justify-between gap-3"><span className="text-muted-foreground">{t('settings.probeTestTokens')}</span><span className="font-mono text-foreground">{result.prompt_tokens ?? '—'} / {result.completion_tokens ?? '—'}</span></div>
            </div>
            {!result.success && (result.error_kind || result.error_message) ? (
              <div className="mt-3 border-t border-border pt-3 text-xs text-destructive">
                <div>{result.error_kind ?? '—'}</div>
                <div className="mt-1 break-all text-muted-foreground">{result.error_message ?? ''}</div>
              </div>
            ) : null}
          </div>
        ) : null}
      </CardContent>
    </Card>
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
