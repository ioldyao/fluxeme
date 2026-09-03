import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@fluxeme/shared/src/components/ui/dialog';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Label } from '@fluxeme/shared/src/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { Checkbox } from '@fluxeme/shared/src/components/ui/checkbox';
import { Switch } from '@fluxeme/shared/src/components/ui/switch';
import { Plus, X } from 'lucide-react';
import { useChannelHealth } from '@fluxeme/shared/src/api/balancer';
import type { Channel, Endpoint } from '@fluxeme/shared/src/types';

import { PROVIDERS, PROVIDER_DISPLAY } from "@fluxeme/shared/src/constants/providers";

const FIXED_BASE_URLS: Record<string, string> = {
  deepseek: 'https://api.deepseek.com',
  // DashScope: URL is auto-generated from region + workspaceId (pure domain; backend
  // appends /apps/anthropic or /compatible-mode/v1 per request kind)
  dashscope: '',
  aiionly: 'https://llm.aiionly.com',
  zhipu: 'https://open.bigmodel.cn/api/paas/v4',
  minimax: 'https://api.minimaxi.com/v1',
};

// DashScope regions for dropdown selection
const DASHSCOPE_REGIONS = [
  { value: 'cn-beijing', label: '华北2（北京）cn-beijing' },
  { value: 'ap-southeast-1', label: '新加坡 ap-southeast-1' },
  { value: 'us-east-1', label: '美国（弗吉尼亚）us-east-1' },
  { value: 'eu-central-1', label: '德国（法兰克福）eu-central-1' },
  { value: 'ap-northeast-1', label: '日本（东京）ap-northeast-1' },
];

interface Props {
  channel?: Channel | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: Record<string, unknown>) => void;
  isPending?: boolean;
}

function emptyEp(): Endpoint {
  return { url: '', api_key: '', weight: 1, timeout_secs: 30, enabled: true, full_url: false };
}

export function ChannelForm({ channel, open, onOpenChange, onSubmit, isPending }: Props) {
  const { t } = useTranslation();
  const { data: health } = useChannelHealth(channel?.id ?? '');
  const [name, setName] = useState('');
  const [provider, setProvider] = useState('');
  const [priority, setPriority] = useState('0');
  const [enabled, setEnabled] = useState(true);
  const [anthropicCompat, setAnthropicCompat] = useState(false);
  const [endpoints, setEndpoints] = useState<Endpoint[]>([emptyEp()]);
  // DashScope-specific fields
  const [dashscopeRegion, setDashscopeRegion] = useState('cn-beijing');
  const [dashscopeWorkspaceId, setDashscopeWorkspaceId] = useState('');
  const [dashscopeType, setDashscopeType] = useState<'dashscope' | 'token_plan'>('dashscope');
  // Qianfan-specific fields: 千帆大模型 类型（普通 千帆大模型 / Token Plan）
  const [qianfanType, setQianfanType] = useState<'qianfan' | 'token_plan'>('qianfan');
  // Volces-specific fields: 火山方舟类型
  const [volcesType, setVolcesType] = useState<'ark' | 'agent_plan' | 'coding_plan'>('ark');

  // DashScope URL construction (auto based on type + region; Token Plan has a
  // fixed `token-plan` prefix, DashScope uses the workspace id)
  const isDashScope = provider === 'dashscope';
  const dashscopeBaseUrl = isDashScope
    ? dashscopeType === 'token_plan'
      ? `https://token-plan.${dashscopeRegion}.maas.aliyuncs.com`
      : dashscopeWorkspaceId
        ? `https://${dashscopeWorkspaceId}.${dashscopeRegion}.maas.aliyuncs.com`
        : ''
    : '';
  // Qianfan Token Plan 固定域名；后端按请求类型自动拼接
  // /v2/tokenplan/personal（OpenAI）或 /anthropic/tokenplan/personal（Anthropic）
  const qianfanFixedBaseUrl =
    provider === 'qianfan' && qianfanType === 'token_plan'
      ? 'https://qianfan.baidubce.com'
      : '';
  // Volces Agent/Coding Plan 固定域名；后端自动拼接 /api/plan/v3/... 或 /api/coding/v3/...
  const volcesPlanBaseUrl =
    provider === 'volces_ark' && volcesType !== 'ark'
      ? 'https://ark.cn-beijing.volces.com'
      : '';
  const fixedBaseUrl = isDashScope
    ? dashscopeBaseUrl
    : qianfanFixedBaseUrl || volcesPlanBaseUrl || FIXED_BASE_URLS[provider] || '';

  useEffect(() => {
    if (channel) {
      setName(channel.name);
      // 千帆 Token Plan 渠道实际 provider 为 qianfan_token_plan，
      // 表单里显示为「千帆大模型」+ 类型「Token Plan」。
      if (channel.provider === 'qianfan_token_plan') {
        setProvider('qianfan');
        setQianfanType('token_plan');
      } else if (channel.provider === 'volces_agent_plan') {
        setProvider('volces_ark');
        setVolcesType('agent_plan');
      } else if (channel.provider === 'volces_coding_plan') {
        setProvider('volces_ark');
        setVolcesType('coding_plan');
      } else {
        setProvider(channel.provider);
        setQianfanType('qianfan');
        setVolcesType('ark');
      }
      setPriority(String(channel.priority));
      setEnabled(channel.enabled);
      setAnthropicCompat(channel.anthropic_compat ?? false);
      // Load DashScope-specific config from channel
      if (channel.provider === 'dashscope') {
        const existingUrl = channel.endpoints[0]?.url || '';
        const match = existingUrl.match(/https:\/\/([^.]+)\.([^.]+)\.maas\.aliyuncs\.com/);
        if (match) {
          const isTokenPlan = match[1] === 'token-plan';
          setDashscopeType(isTokenPlan ? 'token_plan' : 'dashscope');
          setDashscopeWorkspaceId(isTokenPlan ? '' : match[1]);
          setDashscopeRegion(match[2]);
        } else {
          setDashscopeType('dashscope');
          setDashscopeWorkspaceId('');
          setDashscopeRegion('cn-beijing');
        }
      }
      setEndpoints(channel.endpoints.length ? channel.endpoints : [emptyEp()]);
    } else {
      setName(''); setProvider(''); setPriority('0'); setEnabled(true); setAnthropicCompat(false);
      setDashscopeRegion('cn-beijing');
      setDashscopeWorkspaceId('');
      setDashscopeType('dashscope');
      setQianfanType('qianfan');
      setVolcesType('ark');
      setEndpoints([emptyEp()]);
    }
  }, [channel, open]);

  const updateEp = (i: number, field: keyof Endpoint, value: string | number | boolean | null) =>
    setEndpoints((prev) => prev.map((ep, idx) => idx === i ? { ...ep, [field]: value } : ep));
  const toggleFullUrl = (i: number, fullUrl: boolean) => {
    updateEp(i, 'full_url', fullUrl);
  };
  const addEp = () => setEndpoints((prev) => [...prev, fixedBaseUrl ? { ...emptyEp(), url: fixedBaseUrl } : emptyEp()]);
  const removeEp = (i: number) => setEndpoints((prev) => prev.length > 1 ? prev.filter((_, idx) => idx !== i) : prev);

  useEffect(() => {
    if (fixedBaseUrl) {
      setEndpoints((prev) => prev.map((ep) => ep.full_url ? ep : { ...ep, url: fixedBaseUrl }));
    }
  }, [fixedBaseUrl]);

  function healthStatus(ep: Endpoint): { color: string; title: string } {
    if (!health) return { color: 'bg-secondary', title: t('common.unknown') };
    const item = health.endpoints.find((h) => h.endpoint_id === ep.id);
    if (!item) return { color: 'bg-secondary', title: t('common.unknown') };
    if (!item.enabled) return { color: 'bg-muted-foreground', title: t('common.disabled') };
    return item.available
      ? { color: 'bg-chart-2', title: t('common.active') }
      : { color: 'bg-destructive', title: t('common.meltdown') };
  }

  function randomId() {
    return Math.random().toString(36).substring(2, 10);
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    // 千帆大模型类型 → provider qianfan；Token Plan → provider qianfan_token_plan
    const effectiveProvider =
      provider === 'qianfan' && qianfanType === 'token_plan' ? 'qianfan_token_plan'
      : provider === 'volces_ark' && volcesType === 'agent_plan' ? 'volces_agent_plan'
      : provider === 'volces_ark' && volcesType === 'coding_plan' ? 'volces_coding_plan'
      : provider;
    const data: Record<string, unknown> = {
      name,
      provider: effectiveProvider,
      priority: Number(priority),
      enabled,
      anthropic_compat: provider === 'openai' ? anthropicCompat : false,
      ...(channel ? {} : { id: randomId() }),
      endpoints: endpoints.map((endpoint) => ({
        ...endpoint,
        weight: Number(endpoint.weight),
        timeout_secs: endpoint.timeout_secs ? Number(endpoint.timeout_secs) : null,
      })),
    };

    // DashScope-specific config
    if (provider === 'dashscope') {
      (data as Record<string, unknown>).dashscope_region = dashscopeRegion;
      (data as Record<string, unknown>).dashscope_workspace_id = dashscopeType === 'token_plan' ? 'token-plan' : dashscopeWorkspaceId;
      // Use auto-generated URL (pure domain; backend appends /apps/anthropic or
      // /compatible-mode/v1 per request kind)
      const finalUrl = dashscopeBaseUrl;
      // Update endpoints with the final URL
      (data as Record<string, unknown>).endpoints = (data.endpoints as Endpoint[]).map((ep: Endpoint) => ({
        ...ep,
        url: ep.full_url ? ep.url : finalUrl,
      }));
    }

    // Qianfan Token Plan: URL 固定为 https://qianfan.baidubce.com，
    // 后端自动拼接 /v2/tokenplan/personal 或 /anthropic/tokenplan/personal。
    if (provider === 'qianfan' && qianfanType === 'token_plan') {
      (data as Record<string, unknown>).endpoints = (data.endpoints as Endpoint[]).map((ep: Endpoint) => ({
        ...ep,
        url: ep.full_url ? ep.url : 'https://qianfan.baidubce.com',
      }));
    }

    // Volces Agent/Coding Plan: URL 固定为 https://ark.cn-beijing.volces.com，
    // 后端自动拼接 /api/plan/v3/... 或 /api/coding/v3/...。
    if (provider === 'volces_ark' && volcesType !== 'ark') {
      (data as Record<string, unknown>).endpoints = (data.endpoints as Endpoint[]).map((ep: Endpoint) => ({
        ...ep,
        url: ep.full_url ? ep.url : 'https://ark.cn-beijing.volces.com',
      }));
    }

    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl p-0 gap-0 max-h-[85vh] flex flex-col">
        <DialogHeader className="px-6 py-5 border-b shrink-0">
          <DialogTitle className="text-lg font-semibold">
            {channel ? t('channel.edit') : t('channel.add')}
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="flex flex-col flex-1 min-h-0">
          <div className="flex flex-1 min-h-0">
            <div className="w-64 shrink-0 border-r bg-muted/20 px-5 py-6 space-y-5">
              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.name')}</Label>
                <Input
                  className="h-9 bg-background"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={t('form.channelName')}
                />
              </div>

              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.provider')}</Label>
                <Select
                  value={provider}
                  onValueChange={(v) => {
                    setProvider(v ?? '');
                    // 切换提供商时重置所有 provider 专属状态，避免残留：
                    // 例如 OpenAI 的「兼容 Anthropic」开关在 provider 不再是
                    // openai 时 UI 隐藏但 state 保留，提交时仍会带上旧值。
                    setAnthropicCompat(false);
                    setDashscopeType('dashscope');
                    setQianfanType('qianfan');
                    setVolcesType('ark');
                  }}
                  required
                >
                  <SelectTrigger className="h-9 bg-background"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {PROVIDERS.filter((p) => p !== 'qianfan_token_plan' && p !== 'volces_agent_plan' && p !== 'volces_coding_plan').map((p) => (
                      <SelectItem key={p} value={p}>{PROVIDER_DISPLAY[p] || p}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {provider === 'aiionly' && (
                  <p className="text-[11px] text-muted-foreground leading-tight">{t('channel.aiionlyDesc')}</p>
                )}
              </div>

              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.priority')}</Label>
                <Input
                  className="h-9 bg-background"
                  type="number"
                  value={priority}
                  onChange={(e) => setPriority(e.target.value)}
                />
              </div>

              <label className="flex items-center gap-2 text-sm pt-1">
                <Checkbox checked={enabled} onCheckedChange={(v) => setEnabled(!!v)} />
                {t('form.enabled')}
              </label>

              {provider === 'openai' && (
                <div className="space-y-1 pt-2">
                  <div className="flex items-center justify-between">
                    <Label className="text-sm font-medium">{t('channel.anthropicCompat')}</Label>
                    <Switch
                      checked={anthropicCompat}
                      onCheckedChange={(v) => setAnthropicCompat(!!v)}
                    />
                  </div>
                  <p className="text-[11px] text-muted-foreground leading-tight">
                    {t('channel.anthropicCompatDesc')}
                  </p>
                </div>
              )}

              {isDashScope && (
                <div className="space-y-3 pt-2 border-t border-muted/30 mt-2">
                  <div className="space-y-1.5">
                    <Label className="text-sm font-medium">{t('dashscope.type')}</Label>
                    <Select value={dashscopeType} onValueChange={(v) => { const next = (v ?? 'dashscope') as 'dashscope' | 'token_plan'; setDashscopeType(next); if (next === 'token_plan') setDashscopeWorkspaceId(''); }}>
                      <SelectTrigger className="h-9 bg-background">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="dashscope">{t('dashscope.typeDashscope')}</SelectItem>
                        <SelectItem value="token_plan">{t('dashscope.typeTokenPlan')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="space-y-1.5">
                    <Label className="text-sm font-medium">{t('dashscope.region')}</Label>
                    <Select value={dashscopeRegion} onValueChange={(v) => setDashscopeRegion(v ?? 'cn-beijing')}>
                      <SelectTrigger className="h-9 bg-background">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {DASHSCOPE_REGIONS.map((region) => (
                          <SelectItem key={region.value} value={region.value}>
                            {region.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>

                  {dashscopeType === 'token_plan' ? (
                    <div className="space-y-1.5">
                      <p className="text-[10px] text-muted-foreground leading-tight">
                        {t('dashscope.tokenPlanDesc')}
                      </p>
                    </div>
                  ) : (
                    <div className="space-y-1.5">
                      <Label className="text-sm font-medium">{t('dashscope.workspaceId')}</Label>
                      <Input
                        className="h-9 bg-background"
                        placeholder="例如：ws-xxxxxx"
                        value={dashscopeWorkspaceId}
                        onChange={(e) => setDashscopeWorkspaceId(e.target.value)}
                      />
                      <p className="text-[10px] text-muted-foreground leading-tight">
                        {t('dashscope.workspaceIdDesc')}
                      </p>
                    </div>
                  )}

                  <div className="space-y-1.5">
                    <p className="text-[10px] text-muted-foreground leading-tight">
                      后端自动根据请求类型拼接 /apps/anthropic 或 /compatible-mode/v1
                    </p>
                  </div>
                </div>
              )}

              {provider === 'qianfan' && (
                <div className="space-y-3 pt-2 border-t border-muted/30 mt-2">
                  <div className="space-y-1.5">
                    <Label className="text-sm font-medium">{t('qianfan.type')}</Label>
                    <Select
                      value={qianfanType}
                      onValueChange={(v) => setQianfanType((v ?? 'qianfan') as 'qianfan' | 'token_plan')}
                    >
                      <SelectTrigger className="h-9 bg-background">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="qianfan">{t('qianfan.typeQianfan')}</SelectItem>
                        <SelectItem value="token_plan">{t('qianfan.typeTokenPlan')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  {qianfanType === 'token_plan' ? (
                    <div className="space-y-1.5">
                      <p className="text-[10px] text-muted-foreground leading-tight">
                        {t('qianfan.tokenPlanDesc')}
                      </p>
                    </div>
                  ) : (
                    <div className="space-y-1.5">
                      <p className="text-[10px] text-muted-foreground leading-tight">
                        {t('qianfan.normalDesc')}
                      </p>
                    </div>
                  )}
                </div>
              )}

              {provider === 'volces_ark' && (
                <div className="space-y-3 pt-2 border-t border-muted/30 mt-2">
                  <div className="space-y-1.5">
                    <Label className="text-sm font-medium">{t('volces.type')}</Label>
                    <Select
                      value={volcesType}
                      onValueChange={(v) => setVolcesType((v ?? 'ark') as 'ark' | 'agent_plan' | 'coding_plan')}
                    >
                      <SelectTrigger className="h-9 bg-background">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="ark">{t('volces.typeArk')}</SelectItem>
                        <SelectItem value="agent_plan">{t('volces.typeAgentPlan')}</SelectItem>
                        <SelectItem value="coding_plan">{t('volces.typeCodingPlan')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <p className="text-[10px] text-muted-foreground leading-tight">
                    {volcesType === 'ark'
                      ? t('volces.normalDesc')
                      : volcesType === 'agent_plan'
                        ? t('volces.agentPlanDesc')
                        : t('volces.codingPlanDesc')}
                  </p>
                </div>
              )}
            </div>

            <div className="flex-1 min-h-0 flex flex-col">
              <div className="flex items-center justify-between px-6 pt-5 pb-3 shrink-0">
                <Label className="text-sm font-medium text-muted-foreground">
                  {t('form.endpoints')}（{endpoints.length}）
                </Label>
                <Button type="button" variant="ghost" size="sm" className="h-7 text-xs" onClick={addEp}>
                  <Plus className="h-3.5 w-3.5 mr-1" />{t('common.add')}
                </Button>
              </div>

              <div className="flex-1 overflow-y-auto px-6 pb-6 space-y-3">
                {endpoints.map((ep, i) => {
                  const hs = healthStatus(ep);
                  return (
                    <div key={i} className="rounded-lg border bg-muted/30 p-4 space-y-3">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-muted-foreground">端点 {i + 1}</span>
                        <div className="flex items-center gap-3">
                          <span className="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
                            <span className={`inline-block w-2 h-2 rounded-full ${hs.color}`} />
                            {hs.title}
                          </span>
                          <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                            <Checkbox
                              checked={ep.enabled !== false}
                              onCheckedChange={(v) => updateEp(i, 'enabled', !!v)}
                            />
                            {t('form.enabled')}
                          </label>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-destructive"
                            onClick={() => removeEp(i)}
                            disabled={endpoints.length <= 1}
                          >
                            <X className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      </div>

                      <div className="flex items-center justify-between rounded-md border bg-background px-3 py-2">
                        <div>
                          <Label className="text-xs font-medium">{t('form.fullUrl')}</Label>
                          <p className="text-[10px] text-muted-foreground">{t('form.fullUrlDesc')}</p>
                        </div>
                        <Switch
                          checked={ep.full_url ?? false}
                          onCheckedChange={(value) => toggleFullUrl(i, !!value)}
                        />
                      </div>
                      {(!fixedBaseUrl || ep.full_url) && (
                        <Input
                          className="h-9 bg-background"
                          placeholder={ep.full_url ? t('form.fullUrlPlaceholder') : 'URL'}
                          value={ep.url}
                          onChange={(e) => updateEp(i, 'url', e.target.value)}
                          required
                        />
                      )}
                      {fixedBaseUrl && !ep.full_url && (
                        <div className="p-2.5 rounded-md bg-muted/50 text-xs text-muted-foreground">
                          {t('channel.baseUrl')}: <code className="text-xs font-mono">{fixedBaseUrl}</code>
                        </div>
                      )}

                      <div className="grid grid-cols-[1fr_80px_80px] gap-3">
                        <div className="space-y-1">
                          <Input
                            className="h-9 bg-background"
                            placeholder="API Key"
                            type="password"
                            value={ep.api_key}
                            onChange={(e) => updateEp(i, 'api_key', e.target.value)}
                            required={!channel || !ep.id}
                          />
                        </div>
                        <div className="space-y-1">
                          <Input
                            className="h-9 bg-background"
                            placeholder={t('form.weight')}
                            type="number"
                            value={ep.weight}
                            onChange={(e) => updateEp(i, 'weight', Number(e.target.value))}
                          />
                          <p className="text-[10px] text-muted-foreground leading-tight">权重越高流量越多</p>
                        </div>
                        <div className="space-y-1">
                          <Input
                            className="h-9 bg-background"
                            placeholder={t('form.timeout')}
                            type="number"
                            value={ep.timeout_secs ?? ''}
                            onChange={(e) =>
                              updateEp(i, 'timeout_secs', e.target.value ? Number(e.target.value) : null)
                            }
                          />
                          <p className="text-[10px] text-muted-foreground leading-tight">超时秒数</p>
                        </div>
                      </div>
                      <div className="flex gap-3 text-[10px] text-muted-foreground">
                        <span className="inline-flex items-center gap-1">
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-chart-2" /> 正常
                        </span>
                        <span className="inline-flex items-center gap-1">
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-destructive" /> 熔断
                        </span>
                        <span className="inline-flex items-center gap-1">
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-muted-foreground" /> 已禁用
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          <div className="flex justify-end gap-3 px-6 py-4 border-t bg-muted/20 shrink-0">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={isPending}>
              {t('common.save')}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
