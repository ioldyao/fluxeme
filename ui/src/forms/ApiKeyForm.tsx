import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useCreateApiKey } from '@/api/apiKeys';
import { CopyButton } from '@/components/CopyButton';
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import { Card } from '@/components/ui/card';
import { toast } from 'sonner';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ApiKeyForm({ open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const create = useCreateApiKey();
  const [name, setName] = useState('');
  const [enabled, setEnabled] = useState(true);
  const [createdKey, setCreatedKey] = useState<string | null>(null);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    create.mutate({ name: name || null, enabled }, {
      onSuccess: (data) => {
        setCreatedKey(data.key);
        toast.success(t('apikey.generatedTitle'));
      },
      onError: (err) => toast.error(err.message),
    });
  };

  const handleClose = () => {
    setCreatedKey(null);
    setName('');
    setEnabled(true);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={(open) => { if (!open) handleClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{createdKey ? t('apikey.generatedTitle') : t('apikey.add')}</DialogTitle>
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
            <div className="flex justify-end gap-2">
              <Button type="button" variant="outline" onClick={handleClose}>{t('common.cancel')}</Button>
              <Button type="submit" disabled={create.isPending}>{t('common.save')}</Button>
            </div>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
