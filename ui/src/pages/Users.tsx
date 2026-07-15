import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useUsers, useCreateUser, useUpdateUser, useDeleteUser } from '@/api/users';
import { UserForm } from '@/forms/UserForm';
import { PageHeader } from '@/components/PageHeader';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@/components/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardContent } from '@/components/ui/card';
import { Pencil, Trash2, Plus, Search, RefreshCw } from 'lucide-react';
import { toast } from 'sonner';
import type { User } from '@/types';

export default function Users() {
  const { t } = useTranslation();
  const { data: users, isLoading, isError, refetch } = useUsers();
  const createUser = useCreateUser();
  const deleteUser = useDeleteUser();
  const [search, setSearch] = useState('');
  const [editUser, setEditUser] = useState<User | null>(null);
  const updateUser = useUpdateUser(editUser?.id ?? '');
  const [showAdd, setShowAdd] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);

  const filtered = users?.filter((u) => u.id.includes(search) || u.name.includes(search));

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteUser.mutate(deleteTarget.id, {
      onSuccess: () => { toast.success(t('toast.deleted')); setDeleteTarget(null); refetch(); },
      onError: (err) => toast.error(err.message),
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('user.title')}
        description={t('user.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="size-4 mr-1" />{t('user.add')}
            </Button>
          </>
        }
      />
      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input className="pl-9 max-w-xs" placeholder="Search..." value={search} onChange={(e) => setSearch(e.target.value)} />
      </div>
      <Card>
        <CardContent className="p-0">
          {isLoading ? (
            <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
          ) : isError ? (
            <div className="flex items-center justify-center p-8">
              <div className="text-center">
                <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
                <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
              </div>
            </div>
          ) : filtered && filtered.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('table.id')}</th>
                    <th className="text-left py-3 px-4">{t('table.name')}</th>
                    <th className="text-left py-3 px-4">{t('table.role')}</th>
                    <th className="text-left py-3 px-4">{t('table.rateLimits')}</th>
                    <th className="text-right py-3 px-4">{t('table.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((user) => (
                    <tr key={user.id} className="border-b last:border-0 hover:bg-muted/50">
                      <td className="py-3 px-4 font-mono text-xs">{user.id}</td>
                      <td className="py-3 px-4">{user.name}</td>
                      <td className="py-3 px-4">
                        {user.role === 'admin' ? (
                          <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs font-medium bg-brand/10 text-brand">{user.role}</span>
                        ) : (
                          <span className="text-xs text-muted-foreground">{user.role ?? 'user'}</span>
                        )}
                      </td>
                      <td className="py-3 px-4 text-muted-foreground text-xs">
                        {user.rate_limits ? `RPM: ${user.rate_limits.rpm ?? '-'} / TPM: ${user.rate_limits.tpm ?? '-'}` : '-'}
                      </td>
                      <td className="py-3 px-4 text-right">
                        <Button variant="ghost" size="sm" onClick={() => setEditUser(user)}>
                          <Pencil className="size-3.5" />
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleteTarget(user)}>
                          <Trash2 className="size-3.5 text-destructive" />
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <EmptyState message={t('empty.noUsers')} />
          )}
        </CardContent>
      </Card>
      {(showAdd || editUser) && (
        <UserForm
          user={editUser}
          open={true}
          onOpenChange={(open) => { if (!open) { setShowAdd(false); setEditUser(null); }}}
          onSubmit={(data: any) => {
            if (editUser) {
              updateUser.mutate(data, {
                onSuccess: () => { toast.success(t('toast.updated')); setEditUser(null); },
                onError: (err) => toast.error(err.message),
              });
            } else {
              createUser.mutate(data, {
                onSuccess: () => { toast.success(t('toast.created')); setShowAdd(false); },
                onError: (err) => toast.error(err.message),
              });
            }
          }}
          isPending={createUser.isPending || updateUser.isPending}
        />
      )}
      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={() => setDeleteTarget(null)}
        title={t('common.delete')}
        description={`${t('confirm.deleteUser')}${deleteTarget?.id}${t('confirm.suffix')}`}
        onConfirm={handleDelete}
      />
    </div>
  );
}
