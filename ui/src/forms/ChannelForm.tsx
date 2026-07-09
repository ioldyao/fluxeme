import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Plus, X } from 'lucide-react';
import type { Channel, Endpoint } from '@/types';

const PROVIDERS = ['openai', 'anthropic', 'vllm', 'azure', 'ollama'] as const;

interface Props {
  channel?: Channel | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: Record<string, unknown>) => void;
  isPending?: boolean;
}

function emptyEp(): Endpoint {
  return { url: '', api_key: '', weight: 1, timeout_secs: 30 };
}

export function ChannelForm({ channel, open, onOpenChange, onSubmit, isPending }: Props) {
  const { t } = useTranslation();
  const [provider, setProvider] = useState('');
  const [priority, setPriority] = useState('0');
  const [enabled, setEnabled] = useState(true);
  const [endpoints, setEndpoints] = useState<Endpoint[]>([emptyEp()]);

  useEffect(() => {
    if (channel) {
      setProvider(channel.provider);
      setPriority(String(channel.priority));
      setEnabled(channel.enabled);
      setEndpoints(channel.endpoints.length ? channel.endpoints : [emptyEp()]);
    } else {
      setProvider(''); setPriority('0'); setEnabled(true); setEndpoints([emptyEp()]);
    }
  }, [channel, open]);

  const updateEp = (i: number, field: keyof Endpoint, value: string | number | null) =>
    setEndpoints(endpoints.map((ep, idx) => idx === i ? { ...ep, [field]: value } : ep));
  const addEp = () => setEndpoints([...endpoints, emptyEp()]);
  const removeEp = (i: number) => endpoints.length > 1 && setEndpoints(endpoints.filter((_, idx) => idx !== i));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = {
      provider,
      priority: Number(priority),
      enabled,
      ...(channel ? {} : { id: provider }),
      endpoints: endpoints.map((ep) => ({
        ...ep,
        weight: Number(ep.weight),
        timeout_secs: ep.timeout_secs ? Number(ep.timeout_secs) : null,
      })),
    };
    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader><DialogTitle>{channel ? t('channel.edit') : t('channel.add')}</DialogTitle></DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label>{t('form.provider')}</Label>
            <Select value={provider} onValueChange={(v) => setProvider(v ?? '')} required>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                {PROVIDERS.map((p) => (
                  <SelectItem key={p} value={p} className="capitalize">{p}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label>{t('form.priority')}</Label>
              <Input type="number" value={priority} onChange={(e) => setPriority(e.target.value)} />
            </div>
            <div className="flex items-end pb-2 gap-2">
              <Checkbox id="enabled" checked={enabled} onCheckedChange={(v) => setEnabled(!!v)} />
              <Label htmlFor="enabled">{t('form.enabled')}</Label>
            </div>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>{t('form.endpoints')}</Label>
              <Button type="button" variant="ghost" size="sm" onClick={addEp}>
                <Plus className="h-3 w-3 mr-1" />{t('common.add')}
              </Button>
            </div>
            {endpoints.map((ep, i) => (
              <div key={i} className="flex gap-2 items-start border p-2 rounded-md">
                <div className="flex-1 space-y-1">
                  <Input placeholder="URL" value={ep.url} onChange={(e) => updateEp(i, 'url', e.target.value)} required />
                  <Input placeholder="API Key" type="password" value={ep.api_key} onChange={(e) => updateEp(i, 'api_key', e.target.value)} required />
                </div>
                <div className="w-20 space-y-1">
                  <Input placeholder={t('form.weight')} type="number" value={ep.weight} onChange={(e) => updateEp(i, 'weight', Number(e.target.value))} />
                  <Input placeholder={t('form.timeout')} type="number" value={ep.timeout_secs ?? ''} onChange={(e) => updateEp(i, 'timeout_secs', e.target.value ? Number(e.target.value) : null)} />
                </div>
                <Button type="button" variant="ghost" size="sm" className="mt-1" onClick={() => removeEp(i)} disabled={endpoints.length <= 1}>
                  <X className="h-3 w-3" />
                </Button>
              </div>
            ))}
          </div>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={isPending}>{t('common.save')}</Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
