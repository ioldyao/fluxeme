import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useCreateUser,
  usePermanentDeleteUser,
  useRestoreUser,
  useSuspendUser,
  useUpdateUser,
  useUsers,
} from '@fluxeme/shared/src/api/users';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@fluxeme/shared/src/components/ui/tabs';
import { UserForm } from '@/forms/UserForm';
import { Ban, Pencil, Plus, RefreshCw, RotateCcw, Search, Trash2 } from 'lucide-react';
import { toast } from 'sonner';
import type { CreateUserReq, UpdateUserReq, User, UserStatus } from '@fluxeme/shared/src/types';

type ConfirmAction =
  | { type: 'suspend'; user: User }
  | { type: 'restore'; user: User }
  | { type: 'delete'; user: User }
  | null;

function formatSuspendedAt(value?: string | null): string {
  if (!value) {
    return '-';
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return date.toLocaleString();
}

export default function Users() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<UserStatus>('active');
  const [search, setSearch] = useState('');
  const [editUser, setEditUser] = useState<User | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [confirmAction, setConfirmAction] = useState<ConfirmAction>(null);

  const activeUsersQuery = useUsers('active', activeTab === 'active');
  const suspendedUsersQuery = useUsers('suspended', activeTab === 'suspended');
  const createUser = useCreateUser();
  const suspendUser = useSuspendUser();
  const restoreUser = useRestoreUser();
  const permanentDeleteUser = usePermanentDeleteUser();
  const updateUser = useUpdateUser(editUser?.id ?? '');

  const currentQuery = activeTab === 'active' ? activeUsersQuery : suspendedUsersQuery;
  const currentUsers = currentQuery.data ?? [];
  const normalizedSearch = search.trim().toLowerCase();
  const filteredUsers = currentUsers.filter((user) => {
    if (!normalizedSearch) {
      return true;
    }

    return (
      user.id.toLowerCase().includes(normalizedSearch) ||
      user.name.toLowerCase().includes(normalizedSearch)
    );
  });

  const handleRefresh = () => {
    void currentQuery.refetch();
  };

  const handleConfirmAction = async () => {
    if (!confirmAction) {
      return;
    }

    const { user } = confirmAction;

    try {
      if (confirmAction.type === 'suspend') {
        await suspendUser.mutateAsync(user.id);
        toast.success(t('toast.updated'));
        setConfirmAction(null);
        return;
      }

      if (confirmAction.type === 'restore') {
        await restoreUser.mutateAsync(user.id);
        toast.success(t('toast.updated'));
        setConfirmAction(null);
        return;
      }

      await permanentDeleteUser.mutateAsync(user.id);
      toast.success(t('toast.deleted'));
      setConfirmAction(null);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : t('toast.failed'));
      throw error;
    }
  };

  const confirmTitle = confirmAction
    ? t(
        confirmAction.type === 'suspend'
          ? 'user.suspend'
          : confirmAction.type === 'restore'
            ? 'user.restore'
            : 'user.permanentDelete'
      )
    : '';

  const confirmDescription = confirmAction
    ? t(
        confirmAction.type === 'suspend'
          ? 'confirm.suspendUserMessage'
          : confirmAction.type === 'restore'
            ? 'confirm.restoreUserMessage'
            : 'confirm.permanentDeleteUserMessage',
        { id: confirmAction.user.id }
      )
    : '';

  const isMutationPending =
    createUser.isPending ||
    updateUser.isPending ||
    suspendUser.isPending ||
    restoreUser.isPending ||
    permanentDeleteUser.isPending;

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('user.title')}
        description={t('user.subtitle')}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={handleRefresh}>
              <RefreshCw className="mr-1 size-4" />
              {t('common.refresh')}
            </Button>
            <Button onClick={() => setShowAdd(true)}>
              <Plus className="mr-1 size-4" />
              {t('user.add')}
            </Button>
          </>
        }
      />
      <div className="relative">
        <Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          className="max-w-xs pl-9"
          placeholder={t('common.search')}
          aria-label={t('common.search')}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>
      <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as UserStatus)}>
        <TabsList>
          <TabsTrigger value="active">{t('user.tabActive')}</TabsTrigger>
          <TabsTrigger value="suspended">{t('user.tabSuspended')}</TabsTrigger>
        </TabsList>
        <Card>
          <CardContent className="p-0">
            <TabsContent value="active" className="mt-0">
              {activeUsersQuery.isLoading ? (
                <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
              ) : activeUsersQuery.isError ? (
                <div className="flex items-center justify-center p-8">
                  <div className="text-center">
                    <p className="mb-2 text-destructive">{t('err.loadFailed')}</p>
                    <Button variant="outline" onClick={handleRefresh}>
                      {t('common.refresh')}
                    </Button>
                  </div>
                </div>
              ) : filteredUsers.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="px-4 py-3 text-left">{t('table.id')}</th>
                        <th className="px-4 py-3 text-left">{t('table.name')}</th>
                        <th className="px-4 py-3 text-left">{t('table.role')}</th>
                        <th className="px-4 py-3 text-left">{t('table.rateLimits')}</th>
                        <th className="px-4 py-3 text-center">{t('table.concurrencyLimit')}</th>
                        <th className="px-4 py-3 text-right">{t('table.actions')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredUsers.map((user) => (
                        <tr key={user.id} className="border-b last:border-0 hover:bg-muted/50">
                          <td className="px-4 py-3 font-mono text-xs">{user.id}</td>
                          <td className="px-4 py-3">{user.name}</td>
                          <td className="px-4 py-3">
                            {user.role === 'admin' ? (
                              <span className="inline-flex items-center gap-1 rounded bg-brand/10 px-2 py-0.5 text-xs font-medium text-brand">
                                {user.role}
                              </span>
                            ) : (
                              <span className="text-xs text-muted-foreground">
                                {user.role ?? 'user'}
                              </span>
                            )}
                          </td>
                          <td className="px-4 py-3 text-xs text-muted-foreground">
                            {user.rate_limits
                              ? `RPM: ${user.rate_limits.rpm ?? '-'} / TPM: ${user.rate_limits.tpm ?? '-'}`
                              : '-'}
                          </td>
                          <td className="px-4 py-3 text-center text-xs text-muted-foreground">
                            {user.concurrency_limit ?? 2000}
                          </td>
                          <td className="px-4 py-3 text-right">
                            <Button
                              variant="ghost"
                              size="sm"
                              aria-label={t('common.edit')}
                              onClick={() => setEditUser(user)}
                            >
                              <Pencil className="size-3.5" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              aria-label={t('user.suspend')}
                              onClick={() => setConfirmAction({ type: 'suspend', user })}
                            >
                              <Ban className="size-3.5 text-amber-600" />
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
            </TabsContent>
            <TabsContent value="suspended" className="mt-0">
              {suspendedUsersQuery.isLoading ? (
                <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
              ) : suspendedUsersQuery.isError ? (
                <div className="flex items-center justify-center p-8">
                  <div className="text-center">
                    <p className="mb-2 text-destructive">{t('err.loadFailed')}</p>
                    <Button variant="outline" onClick={handleRefresh}>
                      {t('common.refresh')}
                    </Button>
                  </div>
                </div>
              ) : filteredUsers.length > 0 ? (
                <div className="overflow-x-auto">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b text-muted-foreground">
                        <th className="px-4 py-3 text-left">{t('table.id')}</th>
                        <th className="px-4 py-3 text-left">{t('table.name')}</th>
                        <th className="px-4 py-3 text-left">{t('table.role')}</th>
                        <th className="px-4 py-3 text-left">{t('user.suspendedAt')}</th>
                        <th className="px-4 py-3 text-right">{t('table.actions')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {filteredUsers.map((user) => (
                        <tr key={user.id} className="border-b last:border-0 hover:bg-muted/50">
                          <td className="px-4 py-3 font-mono text-xs">{user.id}</td>
                          <td className="px-4 py-3">{user.name}</td>
                          <td className="px-4 py-3">
                            {user.role === 'admin' ? (
                              <span className="inline-flex items-center gap-1 rounded bg-brand/10 px-2 py-0.5 text-xs font-medium text-brand">
                                {user.role}
                              </span>
                            ) : (
                              <span className="text-xs text-muted-foreground">
                                {user.role ?? 'user'}
                              </span>
                            )}
                          </td>
                          <td className="px-4 py-3 text-xs text-muted-foreground">
                            {formatSuspendedAt(user.suspended_at)}
                          </td>
                          <td className="px-4 py-3 text-right">
                            <Button
                              variant="ghost"
                              size="sm"
                              aria-label={t('user.restore')}
                              onClick={() => setConfirmAction({ type: 'restore', user })}
                            >
                              <RotateCcw className="size-3.5 text-emerald-600" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="sm"
                              aria-label={t('user.permanentDelete')}
                              onClick={() => setConfirmAction({ type: 'delete', user })}
                            >
                              <Trash2 className="size-3.5 text-destructive" />
                            </Button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <EmptyState message={t('empty.noSuspendedUsers')} />
              )}
            </TabsContent>
          </CardContent>
        </Card>
      </Tabs>
      {(showAdd || editUser) && (
        <UserForm
          user={editUser}
          open={true}
          onOpenChange={(open) => {
            if (!open) {
              setShowAdd(false);
              setEditUser(null);
            }
          }}
          onSubmit={(data: CreateUserReq | UpdateUserReq) => {
            if (editUser) {
              updateUser.mutate(data, {
                onSuccess: () => {
                  toast.success(t('toast.updated'));
                  setEditUser(null);
                },
                onError: (error) => toast.error(error.message),
              });
              return;
            }

            createUser.mutate(data as CreateUserReq, {
              onSuccess: () => {
                toast.success(t('toast.created'));
                setShowAdd(false);
              },
              onError: (error) => toast.error(error.message),
            });
          }}
          isPending={isMutationPending}
        />
      )}
      <ConfirmDialog
        open={!!confirmAction}
        onOpenChange={(open) => {
          if (!open) {
            setConfirmAction(null);
          }
        }}
        title={confirmTitle}
        description={confirmDescription}
        onConfirm={handleConfirmAction}
        isPending={suspendUser.isPending || restoreUser.isPending || permanentDeleteUser.isPending}
      />
    </div>
  );
}
