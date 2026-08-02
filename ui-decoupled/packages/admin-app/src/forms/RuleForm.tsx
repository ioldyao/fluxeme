import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useChannels } from '@shared/api/channels';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@shared/components/ui/dialog';
import { Button } from '@shared/components/ui/button';
import { Input } from '@shared/components/ui/input';
import { Label } from '@shared/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@shared/components/ui/select';
import { Switch } from '@shared/components/ui/switch';
import type { RoutingRule } from '@shared/types';

interface Props {
  rule?: RoutingRule | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: Record<string, unknown>) => void;
  isPending?: boolean;
}

export function RuleForm({ rule, open, onOpenChange, onSubmit, isPending }: Props) {
  const { t } = useTranslation();
  const { data: channels } = useChannels();
  const [name, setName] = useState('');
  const [sourceModel, setSourceModel] = useState('');
  const [targetModel, setTargetModel] = useState('');
  const [channelId, setChannelId] = useState('');
  const [upstreamModel, setUpstreamModel] = useState('');
  const [userId, setUserId] = useState('');
  const [priority, setPriority] = useState('0');
  const [enabled, setEnabled] = useState(true);
  const [description, setDescription] = useState('');

  useEffect(() => {
    if (rule) {
      setName(rule.name);
      setSourceModel(rule.source_model);
      setTargetModel(rule.target_model);
      setChannelId(rule.channel_id);
      setUpstreamModel(rule.upstream_model);
      setUserId(rule.user_id);
      setPriority(String(rule.priority));
      setEnabled(rule.enabled);
      setDescription(rule.description);
    } else {
      setName('');
      setSourceModel('*');
      setTargetModel('');
      setChannelId('');
      setUpstreamModel('');
      setUserId('*');
      setPriority('0');
      setEnabled(true);
      setDescription('');
    }
  }, [rule, open]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = {
      name,
      source_model: sourceModel,
      target_model: targetModel,
      channel_id: channelId,
      upstream_model: upstreamModel,
      user_id: userId,
      priority: Number(priority),
      enabled,
      description,
    };
    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-xl">{rule ? t('rule.edit') : t('rule.add')}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-6">
          {!rule && (
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.ruleName')}</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} required />
            </div>
          )}

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.sourceModel')} *</Label>
              <Input value={sourceModel} onChange={(e) => setSourceModel(e.target.value)} placeholder="*" />
              <p className="text-xs text-muted-foreground">支持 * 通配符，如 claude-*</p>
            </div>
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.targetModel')}</Label>
              <Input value={targetModel} onChange={(e) => setTargetModel(e.target.value)} placeholder="留空则不改写" />
              <p className="text-xs text-muted-foreground">改写后的模型名</p>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.channel')}</Label>
              <Select value={channelId} onValueChange={(v) => setChannelId(v ?? '')}>
                <SelectTrigger className="h-10">
                  <SelectValue placeholder={t('form.selectChannel')} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="">由模型控制台决定</SelectItem>
                  {channels?.map((ch) => (
                    <SelectItem key={ch.id} value={ch.id}>{ch.id} ({ch.provider})</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.upstreamModel')}</Label>
              <Input value={upstreamModel} onChange={(e) => setUpstreamModel(e.target.value)} placeholder="留空则用模型名" />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('table.userId')}</Label>
              <Input value={userId} onChange={(e) => setUserId(e.target.value)} placeholder="*" />
            </div>
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.priority')}</Label>
              <Input type="number" value={priority} onChange={(e) => setPriority(e.target.value)} />
            </div>
          </div>

          <div className="space-y-2">
            <Label className="text-sm font-medium">{t('form.description')}</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>

          <div className="flex items-center justify-between">
            <Label className="text-sm font-medium">{t('form.enabled')}</Label>
            <Switch checked={enabled} onCheckedChange={setEnabled} />
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
