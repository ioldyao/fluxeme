import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Switch } from '@/components/ui/switch';
import { Plus, X } from 'lucide-react';
import { useChannelHealth } from '@/api/balancer';
import type { Channel, Endpoint } from '@/types';

import { PROVIDERS, PROVIDER_DISPLAY } from "@/constants/providers";

const FIXED_BASE_URLS: Record<string, string> = {
  deepseek: 'https://api.deepseek.com',
  dashscope: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
  zhipu: 'https://open.bigmodel.cn/api/paas/v4',
  minimax: 'https://api.minimaxi.com/v1',
};

interface Props {
  channel?: Channel | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: Record<string, unknown>) => void;
  isPending?: boolean;
}

function emptyEp(): Endpoint {
  return { url: '', api_key: '', weight: 1, timeout_secs: 30, enabled: true };
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
  const fixedBaseUrl = FIXED_BASE_URLS[provider];

  useEffect(() => {
    if (channel) {
      setName(channel.name);
      setProvider(channel.provider);
      setPriority(String(channel.priority));
      setEnabled(channel.enabled);
      setAnthropicCompat(channel.anthropic_compat ?? false);
      setEndpoints(channel.endpoints.length ? channel.endpoints : [emptyEp()]);
    } else {
      setName(''); setProvider(''); setPriority('0'); setEnabled(true); setAnthropicCompat(false); setEndpoints([emptyEp()]);
    }
  }, [channel, open]);

  const updateEp = (i: number, field: keyof Endpoint, value: string | number | boolean | null) =>
    setEndpoints((prev) => prev.map((ep, idx) => idx === i ? { ...ep, [field]: value } : ep));
  const addEp = () => setEndpoints((prev) => [...prev, fixedBaseUrl ? { ...emptyEp(), url: fixedBaseUrl } : emptyEp()]);
  const removeEp = (i: number) => setEndpoints((prev) => prev.length > 1 ? prev.filter((_, idx) => idx !== i) : prev);

  useEffect(() => {
    if (fixedBaseUrl) {
      setEndpoints((prev) => prev.map((ep) => ({ ...ep, url: fixedBaseUrl })));
    }
  }, [fixedBaseUrl]);

  function healthStatus(ep: Endpoint): { color: string; title: string } {
    if (!health) return { color: 'bg-gray-300', title: t('common.unknown') };
    const item = health.endpoints.find((h) => h.endpoint_id === ep.id);
    if (!item) return { color: 'bg-gray-300', title: t('common.unknown') };
    if (!item.enabled) return { color: 'bg-gray-400', title: t('common.disabled') };
    return item.available
      ? { color: 'bg-green-500', title: t('common.active') }
      : { color: 'bg-red-500', title: t('common.meltdown') };
  }

  function randomId() {
    return Math.random().toString(36).substring(2, 10);
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = {
      name,
      provider,
      priority: Number(priority),
      enabled,
      anthropic_compat: anthropicCompat,
      ...(channel ? {} : { id: randomId() }),
      endpoints: endpoints.map((endpoint) => ({
        ...endpoint,
        weight: Number(endpoint.weight),
        timeout_secs: endpoint.timeout_secs ? Number(endpoint.timeout_secs) : null,
      })),
    };
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
                <Select value={provider} onValueChange={(v) => setProvider(v ?? '')} required>
                  <SelectTrigger className="h-9 bg-background"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {PROVIDERS.map((p) => (
                      <SelectItem key={p} value={p}>{PROVIDER_DISPLAY[p] || p}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
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

                      {!fixedBaseUrl && (
                        <Input
                          className="h-9 bg-background"
                          placeholder="URL"
                          value={ep.url}
                          onChange={(e) => updateEp(i, 'url', e.target.value)}
                          required
                        />
                      )}
                      {fixedBaseUrl && (
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
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-green-500" /> 正常
                        </span>
                        <span className="inline-flex items-center gap-1">
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-red-500" /> 熔断
                        </span>
                        <span className="inline-flex items-center gap-1">
                          <span className="inline-block w-1.5 h-1.5 rounded-full bg-gray-400" /> 已禁用
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
