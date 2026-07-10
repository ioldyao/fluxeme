import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import type { User, CreateUserReq, UpdateUserReq } from '@/types';

interface Props {
  user?: User | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (data: CreateUserReq | UpdateUserReq) => void;
  isPending?: boolean;
}

export function UserForm({ user, open, onOpenChange, onSubmit, isPending }: Props) {
  const { t } = useTranslation();
  const [id, setId] = useState('');
  const [name, setName] = useState('');
  const [password, setPassword] = useState('');
  const [rpm, setRpm] = useState('');
  const [tpm, setTpm] = useState('');

  useEffect(() => {
    if (user) {
      setId(user.id);
      setName(user.name);
      setRpm(String(user.rate_limits?.rpm ?? ''));
      setTpm(String(user.rate_limits?.tpm ?? ''));
    } else {
      setId(''); setName(''); setPassword(''); setRpm(''); setTpm('');
    }
  }, [user, open]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const rateLimits = rpm || tpm
      ? { rpm: rpm ? Number(rpm) : null, tpm: tpm ? Number(tpm) : null }
      : undefined;

    if (user) {
      const data: UpdateUserReq = {};
      if (name !== user.name) data.name = name;
      if (password) data.password = password;
      if (rateLimits) data.rate_limits = rateLimits;
      onSubmit(data);
    } else {
      onSubmit({
        id, name,
        password: password || null,
        rate_limits: rateLimits ?? null,
      });
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle className="text-xl">{user ? t('user.edit') : t('user.add')}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-6">
          {user ? (
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.userId')}</Label>
              <div className="px-3 py-2 rounded-md bg-muted text-sm text-muted-foreground">{user.id}</div>
            </div>
          ) : (
            <div className="space-y-2">
              <Label className="text-sm font-medium">{t('form.userId')}</Label>
              <Input value={id} onChange={(e) => setId(e.target.value)} required />
            </div>
          )}
          <div className="space-y-2">
            <Label className="text-sm font-medium">{t('form.name')}</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} required />
          </div>
          <div className="space-y-2">
            <Label className="text-sm font-medium">
              {t('login.password')}{user && <span className="text-muted-foreground font-normal ml-1">（留空不修改）</span>}
            </Label>
            <Input type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
          </div>
          <div className="space-y-3">
            <Label className="text-sm font-medium">速率限制</Label>
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label className="text-xs text-muted-foreground">{t('form.rpm')}</Label>
                <Input type="number" value={rpm} onChange={(e) => setRpm(e.target.value)} placeholder="不限" />
              </div>
              <div className="space-y-2">
                <Label className="text-xs text-muted-foreground">{t('form.tpm')}</Label>
                <Input type="number" value={tpm} onChange={(e) => setTpm(e.target.value)} placeholder="不限" />
              </div>
            </div>
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
