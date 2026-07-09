import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '@/components/CopyButton';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import { Card } from '@/components/ui/card';
import type { ApiKey, CreateKeyReq } from '@/types';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: CreateKeyReq) => void;
  createdKey?: string | null;
  editKey?: ApiKey | null;
  isPending?: boolean;
}

export function ApiKeyForm({ open, onOpenChange, onSubmit, createdKey, editKey, isPending }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [expiresAt, setExpiresAt] = useState('');
  const [spendLimit, setSpendLimit] = useState('');
  const [allowedModels, setAllowedModels] = useState('');

  const isEdit = !!editKey;

  useEffect(() => {
    if (editKey) {
      setName(editKey.name);
      setEnabled(editKey.enabled);
      setExpiresAt(editKey.expires_at ?? '');
      setSpendLimit(editKey.spend_limit ? String(editKey.spend_limit) : '');
      setAllowedModels(editKey.allowed_models?.join(', ') ?? '');
    } else {
      setName('');
      setEnabled(true);
      setExpiresAt('');
      setSpendLimit('');
      setAllowedModels('');
    }
  }, [editKey, open]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const data: CreateKeyReq = {
      name: name || null,
      enabled,
      expires_at: expiresAt || null,
      spend_limit: spendLimit ? Number(spendLimit) : null,
      allowed_models: allowedModels ? allowedModels.split(',').map((m) => m.trim()).filter(Boolean) : null,
    };
    onSubmit(data);
  };

  const handleClose = () => {
    setName('');
    setEnabled(true);
    setExpiresAt('');
    setSpendLimit('');
    setAllowedModels('');
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={(open) => { if (!open) handleClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{createdKey ? t('apikey.generatedTitle') : isEdit ? '编辑 API Key' : t('apikey.add')}</DialogTitle>
          {createdKey && <DialogDescription>{t('apikey.generatedHint')}</DialogDescription>}
        </DialogHeader>

        {createdKey ? (
          <div className="space-y-4">
            <Card className="p-4">
              <div className="flex items-center gap-2">
                <code className="flex-1 text-sm font-mono break-all">{createdKey}</code>
                <CopyButton text={createdKey} />
              </div>
            </Card>
            <div className="flex justify-end">
              <Button onClick={handleClose}>{t('common.done')}</Button>
            </div>
          </div>
        ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            <div className="space-y-2">
              <Label>{t('apikey.name')}</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={t('apikey.nameHint')} />
            </div>
            <div className="flex items-center gap-2">
              <Checkbox id="keyEnabled" checked={enabled} onCheckedChange={(v) => setEnabled(!!v)} />
              <Label htmlFor="keyEnabled">{t('form.enabled')}</Label>
            </div>
            <div className="space-y-2">
              <Label>过期时间</Label>
              <Input type="datetime-local" value={expiresAt} onChange={(e) => setExpiresAt(e.target.value)} />
              <p className="text-xs text-muted-foreground">留空表示永不过期</p>
            </div>
            <div className="space-y-2">
              <Label>费用限制</Label>
              <Input type="number" step="0.01" value={spendLimit} onChange={(e) => setSpendLimit(e.target.value)} placeholder="不限" />
              <p className="text-xs text-muted-foreground">留空表示无限制</p>
            </div>
            <div className="space-y-2">
              <Label>模型限制</Label>
              <Input value={allowedModels} onChange={(e) => setAllowedModels(e.target.value)} placeholder="gpt-4, gpt-3.5-turbo" />
              <p className="text-xs text-muted-foreground">留空表示无限制，多个模型用逗号分隔</p>
            </div>
            <div className="flex justify-end gap-2">
              <Button type="button" variant="outline" onClick={handleClose}>{t('common.cancel')}</Button>
              <Button type="submit" disabled={isPending}>{t('common.save')}</Button>
            </div>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
