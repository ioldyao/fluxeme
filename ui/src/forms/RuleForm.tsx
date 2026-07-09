import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useChannels } from '@/api/channels';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import type { RoutingRule } from '@/types';

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
  const [userId, setUserId] = useState('');
  const [modelPattern, setModelPattern] = useState('');
  const [channelId, setChannelId] = useState('');

  useEffect(() => {
    if (rule) {
      setName(rule.name);
      setUserId(rule.user_id);
      setModelPattern(rule.model_pattern);
      setChannelId(rule.channel_id);
    } else {
      setName(''); setUserId('*'); setModelPattern('*'); setChannelId('');
    }
  }, [rule, open]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data = { name, user_id: userId, model_pattern: modelPattern, channel_id: channelId };
    onSubmit(data);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{rule ? t('rule.edit') : t('rule.add')}</DialogTitle></DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          {!rule && (
            <div className="space-y-2">
              <Label>{t('form.ruleName')}</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} required />
            </div>
          )}
          <div className="space-y-2">
            <Label>{t('form.userIdLabel')}</Label>
            <Input value={userId} onChange={(e) => setUserId(e.target.value)} />
          </div>
          <div className="space-y-2">
            <Label>{t('form.modelPattern')}</Label>
            <Input value={modelPattern} onChange={(e) => setModelPattern(e.target.value)} placeholder="*" />
          </div>
          <div className="space-y-2">
            <Label>{t('form.channel')}</Label>
            <Select value={channelId} onValueChange={(v) => setChannelId(v ?? '')} required>
              <SelectTrigger><SelectValue placeholder={t('form.selectChannel')} /></SelectTrigger>
              <SelectContent>
                {channels?.map((ch) => (
                  <SelectItem key={ch.id} value={ch.id}>{ch.id} ({ch.provider})</SelectItem>
                ))}
              </SelectContent>
            </Select>
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
