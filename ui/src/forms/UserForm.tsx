import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { useCreateUser, useUpdateUser } from '@/api/users';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { toast } from 'sonner';
import type { User, UpdateUserReq } from '@/types';

interface Props {
  user?: User | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function UserForm({ user, open, onOpenChange }: Props) {
  const { t } = useTranslation();
  const createUser = useCreateUser();
  const updateUser = useUpdateUser(user?.id ?? '');
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
      updateUser.mutate(data, {
        onSuccess: () => { toast.success(t('toast.updated')); onOpenChange(false); },
        onError: (err) => toast.error(err.message),
      });
    } else {
      createUser.mutate({
        id, name,
        password: password || null,
        rate_limits: rateLimits ?? null,
      }, {
        onSuccess: () => { toast.success(t('toast.created')); onOpenChange(false); },
        onError: (err) => toast.error(err.message),
      });
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>{user ? t('user.edit') : t('user.add')}</DialogTitle></DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          {!user && (
            <div className="space-y-2">
              <Label htmlFor="uid">{t('form.userId')}</Label>
              <Input id="uid" value={id} onChange={(e) => setId(e.target.value)} required />
            </div>
          )}
          <div className="space-y-2">
            <Label htmlFor="uname">{t('form.name')}</Label>
            <Input id="uname" value={name} onChange={(e) => setName(e.target.value)} required />
          </div>
          {user ? (
            <div className="space-y-2">
              <Label htmlFor="upwd">{t('login.password')} (留空不修改)</Label>
              <Input id="upwd" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </div>
          ) : (
            <div className="space-y-2">
              <Label htmlFor="pwd">{t('login.password')}</Label>
              <Input id="pwd" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </div>
          )}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="rpm">{t('form.rpm')}</Label>
              <Input id="rpm" type="number" value={rpm} onChange={(e) => setRpm(e.target.value)} />
            </div>
            <div className="space-y-2">
              <Label htmlFor="tpml">{t('form.tpm')}</Label>
              <Input id="tpml" type="number" value={tpm} onChange={(e) => setTpm(e.target.value)} />
            </div>
          </div>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
            <Button type="submit" disabled={createUser.isPending || updateUser.isPending}>{t('common.save')}</Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}
