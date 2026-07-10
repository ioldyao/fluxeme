import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useChannels } from '@/api/channels';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Plus, X } from 'lucide-react';
import type { Model } from '@/types';

interface Props {
  model?: Model | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: Record<string, unknown>) => void;
  isPending?: boolean;
}

export function ModelForm({ model, open, onOpenChange, onSubmit, isPending }: Props) {
  const { t } = useTranslation();
  const { data: channels } = useChannels();
  const [id, setId] = useState('');
  const [name, setName] = useState('');
  const [modelPattern, setModelPattern] = useState('');
  const [promptPrice, setPromptPrice] = useState('0');
  const [completionPrice, setCompletionPrice] = useState('0');
  const [contextLength, setContextLength] = useState('');
  const [bindings, setBindings] = useState<{ channel_id: string; priority: number }[]>([]);

  useEffect(() => {
    if (model) {
      setId(model.id);
      setName(model.name);
      setModelPattern(model.model_pattern);
      setPromptPrice(String(model.pricing.prompt_price));
      setCompletionPrice(String(model.pricing.completion_price));
      setContextLength(model.context_length ? String(model.context_length) : '');
      setBindings(model.channels);
    } else {
      setId(''); setName(''); setModelPattern(''); setPromptPrice('0'); setCompletionPrice('0'); setContextLength(''); setBindings([]);
    }
  }, [model, open]);

  const addBinding = () => {
    if (!channels?.length) return;
    setBindings([...bindings, { channel_id: channels[0].id, priority: 0 }]);
  };
  const updateBinding = (i: number, field: string, value: string | number) =>
    setBindings(bindings.map((b, idx) => idx === i ? { ...b, [field]: value } : b));
  const removeBinding = (i: number) => setBindings(bindings.filter((_, idx) => idx !== i));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = {
      id, name, model_pattern: modelPattern,
      pricing: { prompt_price: Number(promptPrice), completion_price: Number(completionPrice) },
      context_length: contextLength ? Number(contextLength) : null,
      channels: bindings.map((b) => ({ channel_id: b.channel_id, priority: Number(b.priority) })),
    };
    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-xl">{model ? t('model.edit') : t('model.add')}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-6">
          {!model && (
            <div className="space-y-2">
              <Label>{t('form.modelName')}</Label>
              <Input value={id} onChange={(e) => setId(e.target.value)} placeholder="gpt-4, claude-sonnet-4-20250514" required />
            </div>
          )}
          <div className="space-y-2">
            <Label>{t('form.name')}</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={t('form.namePlaceholder')} required />
          </div>
          <div className="space-y-2">
            <Label>{t('form.modelPattern')}</Label>
            <Input value={modelPattern} onChange={(e) => setModelPattern(e.target.value)} placeholder="gpt-4*, claude-*" />
          </div>
          <div className="space-y-2">
            <Label>{t('form.pricing')}</Label>
            <div className="grid grid-cols-2 gap-2">
              <div>
                <Label className="text-xs">{t('form.promptPrice')}</Label>
                <Input type="number" step="0.0001" value={promptPrice} onChange={(e) => setPromptPrice(e.target.value)} />
              </div>
              <div>
                <Label className="text-xs">{t('form.completionPrice')}</Label>
                <Input type="number" step="0.0001" value={completionPrice} onChange={(e) => setCompletionPrice(e.target.value)} />
              </div>
            </div>
          </div>
          <div className="space-y-2">
            <Label>上下文长度</Label>
            <Input type="number" step="1" min="0" value={contextLength} onChange={(e) => setContextLength(e.target.value)} placeholder="例如: 131072" />
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>{t('form.bindChannels')}</Label>
              <Button type="button" variant="ghost" size="sm" onClick={addBinding} disabled={!channels?.length}>
                <Plus className="h-3 w-3 mr-1" />{t('common.add')}
              </Button>
            </div>
            {!channels?.length && <p className="text-xs text-muted-foreground">{t('form.noChannels')}</p>}
            {bindings.map((b, i) => (
              <div key={i} className="flex gap-2 items-center border p-2 rounded-md">
                <div className="flex-1">
                  <Select value={b.channel_id} onValueChange={(v) => updateBinding(i, 'channel_id', v ?? '')}>
                    <SelectTrigger>
                      <span>{channels?.find((ch) => ch.id === b.channel_id)?.name || b.channel_id}</span>
                      <SelectValue className="sr-only" />
                    </SelectTrigger>
                    <SelectContent>
                      {channels?.map((ch) => (
                        <SelectItem key={ch.id} value={ch.id}>{ch.name || ch.id} ({ch.provider})</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="w-20">
                  <Input type="number" placeholder={t('form.channelPriority')} value={b.priority}
                    onChange={(e) => updateBinding(i, 'priority', Number(e.target.value))} />
                </div>
                <Button type="button" variant="ghost" size="sm" onClick={() => removeBinding(i)}>
                  <X className="h-3 w-3" />
                </Button>
              </div>
            ))}
          </div>
          <div className="flex justify-end gap-3 pt-2">
            <Button type="button" variant="outline" size="lg" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" size="lg" disabled={isPending}>{t('common.save')}</Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
