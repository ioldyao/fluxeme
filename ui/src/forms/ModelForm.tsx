import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Plus, X } from 'lucide-react';
import { useChannels } from '@/api/channels';
import { Checkbox } from '@/components/ui/checkbox';
import type { Model } from '@/types';

const CATEGORY_VALUES = ['chat', 'reasoning', 'tools', 'web', 'vision', 'rerank', 'embedding'] as const;

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
  const [contextLength, setContextLength] = useState('');
  const [bindings, setBindings] = useState<{ channel_id: string; priority: number; upstream_model: string }[]>([]);
  const [category, setCategory] = useState<string[]>([]);

  useEffect(() => {
    if (model) {
      setId(model.id);
      setName(model.name);
      setModelPattern(model.model_pattern);
      setContextLength(model.context_length ? String(model.context_length) : '');
      setBindings(model.channels?.map((c) => ({ ...c, upstream_model: c.upstream_model || '' })) || []);
      setCategory(model.category ? model.category.split(',').filter(Boolean) : []);
    } else {
      setId(''); setName(''); setModelPattern('');
      setContextLength(''); setBindings([]); setCategory([]); setSelectedAddChannel('');
    }
  }, [model, open]);

  const [selectedAddChannel, setSelectedAddChannel] = useState('');
  const availableChannels = channels?.filter((ch) => !bindings.some((b) => b.channel_id === ch.id)) ?? [];

  const addBinding = (channelId: string) => {
    if (!channelId) return;
    setBindings([...bindings, { channel_id: channelId, priority: 0, upstream_model: '' }]);
    setSelectedAddChannel('');
  };
  const updateBinding = (i: number, field: string, value: string | number) =>
    setBindings(bindings.map((b, idx) => idx === i ? { ...b, [field]: value } : b));
  const removeBinding = (i: number) => setBindings(bindings.filter((_, idx) => idx !== i));

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = {
      id: id || name || modelPattern, name, model_pattern: modelPattern,
      pricing: model?.pricing ?? { prompt_price: 0, completion_price: 0, cache_read_price: 0, cache_write_price: 0, image_input_price: 0, audio_input_price: 0, audio_output_price: 0 },
      context_length: contextLength ? Number(contextLength) : null,
      published: model?.published ?? false,
      category: category.join(','),
      channels: bindings.map((b) => ({ channel_id: b.channel_id, priority: Number(b.priority), upstream_model: b.upstream_model || null })),
    };
    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-4xl p-0 gap-0 max-h-[85vh] flex flex-col">
        <DialogHeader className="px-6 py-5 border-b shrink-0">
          <DialogTitle className="text-lg font-semibold">
            {model ? t('model.edit') : t('model.add')}
          </DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="flex flex-col flex-1 min-h-0">
          <div className="flex flex-1 min-h-0">
            <div className="w-72 shrink-0 border-r bg-muted/20 px-5 py-6 space-y-5 overflow-y-auto">
              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.name')}</Label>
                <Input
                  className="h-9 bg-background"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={t('form.namePlaceholder')}
                  required
                />
              </div>

              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.modelPattern')}</Label>
                <Input
                  className="h-9 bg-background"
                  value={modelPattern}
                  onChange={(e) => setModelPattern(e.target.value)}
                  placeholder="gpt-4*, claude-*"
                />
              </div>

              <div className="space-y-1.5">
                <Label className="text-sm font-medium">{t('form.contextLength')}</Label>
                <Input
                  className="h-9 bg-background"
                  type="number"
                  step="1"
                  min="0"
                  value={contextLength}
                  onChange={(e) => setContextLength(e.target.value)}
                  placeholder={t('form.contextLengthPlaceholder')}
                />
              </div>

              <div className="space-y-2 pt-1">
                <Label className="text-xs font-medium text-muted-foreground">{t('model.category')}</Label>
                <div className="grid grid-cols-2 gap-1.5">
                  {CATEGORY_VALUES.map((cat) => (
                    <label key={cat} className="flex items-center gap-1.5 text-xs cursor-pointer select-none">
                      <Checkbox
                        checked={category.includes(cat)}
                        onCheckedChange={(v) => {
                          if (v) {
                            setCategory([...category, cat]);
                          } else {
                            setCategory(category.filter((c) => c !== cat));
                          }
                        }}
                      />
                      {t(`model.category.${cat}`)}
                    </label>
                  ))}
                </div>
              </div>

            </div>

            <div className="flex-1 min-h-0 flex flex-col">
              <div className="flex items-center justify-between px-6 pt-5 pb-3 shrink-0">
                <Label className="text-sm font-medium text-muted-foreground">
                  {t('form.bindChannelsCount', { count: bindings.length })}
                </Label>
                <div className="flex items-center gap-2">
                  <Select value={selectedAddChannel} onValueChange={(v) => setSelectedAddChannel(v ?? '')}>
                    <SelectTrigger className="h-8 w-72 text-xs bg-background">
                      <SelectValue placeholder={t('form.addChannelPlaceholder')} />
                    </SelectTrigger>
                    <SelectContent>
                      {availableChannels.length === 0 ? (
                        <div className="px-2 py-4 text-xs text-center text-muted-foreground">
                          {t('form.noAvailableChannels')}
                        </div>
                      ) : (
                        availableChannels.map((ch) => (
                          <SelectItem key={ch.id} value={ch.id} className="font-mono text-xs">
                            <span className="truncate">{ch.name || ch.id}</span>
                            <span className="shrink-0 text-muted-foreground"> ({ch.provider})</span>
                            <span className="text-muted-foreground"> - </span>
                            <span className="truncate text-muted-foreground">{ch.id}</span>
                          </SelectItem>
                        ))
                      )}
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    size="sm"
                    className="h-8 text-xs"
                    disabled={!selectedAddChannel}
                    onClick={() => addBinding(selectedAddChannel)}
                  >
                    <Plus className="size-3.5 mr-1" />{t('common.add')}
                  </Button>
                </div>
              </div>

              <div className="flex-1 overflow-y-auto px-6 pb-6">
                {!channels?.length && (
                  <p className="text-xs text-muted-foreground">{t('form.noChannels')}</p>
                )}

                {bindings.length > 0 && (
                  <div className="space-y-2">
                    {bindings.map((b, i) => (
                      <div
                        key={i}
                        className="rounded-lg border bg-muted/30 px-3 py-2.5 space-y-2"
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-sm font-medium truncate">
                            {channels?.find((ch) => ch.id === b.channel_id)?.name || b.channel_id}
                          </span>
                          {(() => {
                            const ch = channels?.find((c) => c.id === b.channel_id);
                            return ch ? (
                              <>
                                <span className="text-xs text-muted-foreground shrink-0">{ch.provider}</span>
                                <span className="text-xs text-muted-foreground/60 truncate">{ch.id}</span>
                              </>
                            ) : null;
                          })()}
                          <div className="flex-1" />
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground hover:text-destructive shrink-0"
                            onClick={() => removeBinding(i)}
                          >
                            <X className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="text-xs text-muted-foreground shrink-0">{t('form.upstreamModel') || '上游模型名'}:</span>
                          <Input
                            className="h-8 bg-background text-xs flex-1"
                            placeholder={name || t('form.namePlaceholder')}
                            value={b.upstream_model}
                            onChange={(e) => updateBinding(i, 'upstream_model', e.target.value)}
                          />
                          <span className="text-xs text-muted-foreground shrink-0">{t('form.channelPriority')}:</span>
                          <Input
                            className="h-8 bg-background w-20"
                            type="number"
                            placeholder="0"
                            value={b.priority}
                            onChange={(e) => updateBinding(i, 'priority', Number(e.target.value))}
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                )}
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
