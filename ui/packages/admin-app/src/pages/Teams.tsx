import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useAdminTeams,
  useAdminTeamMembers,
  useCreateTeam,
  useDeleteTeam,
  useAdminAddTeamMember,
  useAdminSetTeamMemberRole,
  useAdminRemoveTeamMember,
  useAdminTeamWallet,
  useTeamWalletTransactions,
} from '@fluxeme/shared/src/api/teams';
import { useRedeemKey } from '@fluxeme/shared/src/api/wallet';
import { ConfirmDialog } from '@/components/ConfirmDialog';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@fluxeme/shared/src/components/ui/tabs';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@fluxeme/shared/src/components/ui/dialog';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@fluxeme/shared/src/components/ui/select';
import { Plus, Trash2, UserPlus, Wallet } from 'lucide-react';
import { toast } from 'sonner';
import type { AdminTeam } from '@fluxeme/shared/src/api/teams';
import type { TeamRole } from '@fluxeme/shared/src/types';

export default function Teams() {
  const { t } = useTranslation();
  const teamsQuery = useAdminTeams();
  const [showCreate, setShowCreate] = useState(false);
  const [newTeamName, setNewTeamName] = useState('');
  const [newOwnerId, setNewOwnerId] = useState('');
  const [selectedTeam, setSelectedTeam] = useState<AdminTeam | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<AdminTeam | null>(null);
  const [tab, setTab] = useState('members');

  const createTeam = useCreateTeam();
  const deleteTeam = useDeleteTeam();

  const handleCreate = () => {
    const name = newTeamName.trim();
    const ownerId = newOwnerId.trim();
    if (!name || !ownerId) {
      toast.error(t('common.required'));
      return;
    }
    createTeam.mutate(
      { name, ownerId },
      {
        onSuccess: () => {
          toast.success(t('toast.created'));
          setShowCreate(false);
          setNewTeamName('');
          setNewOwnerId('');
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const handleDelete = () => {
    if (!deleteTarget) return;
    deleteTeam.mutate(deleteTarget.id, {
      onSuccess: () => {
        toast.success(t('toast.deleted'));
        setDeleteTarget(null);
        if (selectedTeam?.id === deleteTarget.id) setSelectedTeam(null);
      },
      onError: (e) => toast.error(e.message),
    });
  };

  const teams = teamsQuery.data ?? [];

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('team.title')}
        description={t('team.subtitle')}
        actions={
          <Button onClick={() => setShowCreate(true)}>
            <Plus className="mr-1 size-4" />
            {t('team.add')}
          </Button>
        }
      />

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardContent className="p-0">
            {teamsQuery.isLoading ? (
              <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
            ) : teams.length === 0 ? (
              <div className="p-8">
                <EmptyState message={t('team.empty')} />
              </div>
            ) : (
              <div className="divide-y">
                {teams.map((team) => (
                  <div
                    key={team.id}
                    className={`flex cursor-pointer items-center justify-between px-4 py-3 hover:bg-muted/50 ${
                      selectedTeam?.id === team.id ? 'bg-muted/50' : ''
                    }`}
                    onClick={() => setSelectedTeam(team)}
                  >
                    <div>
                      <div className="font-medium">{team.name}</div>
                      <div className="text-xs text-muted-foreground">
                        {team.id} · {team.role || t('team.roleMember')}
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        setDeleteTarget(team);
                      }}
                    >
                      <Trash2 className="size-3.5 text-destructive" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardContent className="p-4">
            {selectedTeam ? (
              <Tabs value={tab} onValueChange={setTab}>
                <TabsList>
                  <TabsTrigger value="members">
                    <UserPlus className="mr-1 size-3.5" />
                    {t('team.tabMembers')}
                  </TabsTrigger>
                  <TabsTrigger value="wallet">
                    <Wallet className="mr-1 size-3.5" />
                    {t('team.tabWallet')}
                  </TabsTrigger>
                </TabsList>
                <div className="mt-4">
                  <div className="mb-3">
                    <h3 className="text-lg font-semibold">{selectedTeam.name}</h3>
                    <p className="text-xs text-muted-foreground">
                      {t('team.owner')}: {selectedTeam.owner_id}
                    </p>
                  </div>
                  <TabsContent value="members" className="mt-0">
                    <MembersTab teamId={selectedTeam.id} />
                  </TabsContent>
                  <TabsContent value="wallet" className="mt-0">
                    <WalletTab teamId={selectedTeam.id} />
                  </TabsContent>
                </div>
              </Tabs>
            ) : (
              <div className="p-8 text-center text-muted-foreground">
                {t('team.members')}
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('team.add')}</DialogTitle>
          </DialogHeader>
          <Input
            value={newTeamName}
            onChange={(e) => setNewTeamName(e.target.value)}
            placeholder={t('team.name')}
            onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
          />
          <Input
            value={newOwnerId}
            onChange={(e) => setNewOwnerId(e.target.value)}
            placeholder={t('team.ownerId')}
            onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
          />
          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={() => setShowCreate(false)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={handleCreate}>{t('common.save')}</Button>
          </div>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={!!deleteTarget}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
        title={t('common.delete')}
        description={t('team.deleteConfirm', { name: deleteTarget?.name ?? '' })}
        onConfirm={handleDelete}
        isPending={deleteTeam.isPending}
      />
    </div>
  );
}

function MembersTab({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const membersQuery = useAdminTeamMembers(teamId);
  const addMember = useAdminAddTeamMember();
  const setRole = useAdminSetTeamMemberRole();
  const removeMember = useAdminRemoveTeamMember();
  const [showAdd, setShowAdd] = useState(false);
  const [newId, setNewId] = useState('');
  const [newRole, setNewRole] = useState<TeamRole>('member');

  const handleAdd = () => {
    if (!newId.trim()) return;
    addMember.mutate(
      { teamId, userId: newId.trim(), role: newRole },
      {
        onSuccess: () => {
          toast.success(t('toast.created'));
          setShowAdd(false);
          setNewId('');
          setNewRole('member');
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const members = membersQuery.data ?? [];
  return (
    <div className="space-y-2">
      <Button size="sm" onClick={() => setShowAdd(true)}>
        <UserPlus className="mr-1 size-4" />
        {t('team.addMember')}
      </Button>
      {membersQuery.isLoading ? (
        <div className="p-4 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : members.length === 0 ? (
        <EmptyState message={t('team.emptyMembers')} />
      ) : (
        members.map((m) => (
          <div
            key={m.user_id}
            className="flex items-center justify-between rounded border px-3 py-2"
          >
            <span className="font-mono text-sm">{m.user_id}</span>
            <div className="flex items-center gap-2">
              <Select
                value={m.role}
                disabled={m.role === 'owner'}
                onValueChange={(role) =>
                  setRole.mutate(
                    { teamId, userId: m.user_id, role: role as TeamRole },
                    { onError: (e) => toast.error(e.message) },
                  )
                }
              >
                <SelectTrigger className="h-8 w-28">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="admin">{t('team.roleAdmin')}</SelectItem>
                  <SelectItem value="member">{t('team.roleMember')}</SelectItem>
                </SelectContent>
              </Select>
              {m.role !== 'owner' && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    removeMember.mutate(
                      { teamId, userId: m.user_id },
                      { onError: (e) => toast.error(e.message) },
                    )
                  }
                >
                  <Trash2 className="size-3.5 text-destructive" />
                </Button>
              )}
            </div>
          </div>
        ))
      )}
      <Dialog open={showAdd} onOpenChange={setShowAdd}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('team.addMember')}</DialogTitle>
          </DialogHeader>
          <Input
            value={newId}
            onChange={(e) => setNewId(e.target.value)}
            placeholder={t('team.memberId')}
          />
          <Select value={newRole} onValueChange={(r) => setNewRole(r as TeamRole)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="admin">{t('team.roleAdmin')}</SelectItem>
              <SelectItem value="member">{t('team.roleMember')}</SelectItem>
            </SelectContent>
          </Select>
          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={() => setShowAdd(false)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={handleAdd}>{t('common.save')}</Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}


function WalletTab({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const walletQuery = useAdminTeamWallet(teamId);
  const txQuery = useTeamWalletTransactions(teamId);
  const redeem = useRedeemKey();
  const [redeemKeyInput, setRedeemKeyInput] = useState('');

  const handleRedeem = () => {
    const key = redeemKeyInput.trim();
    if (!key) {
      toast.error(t('common.required'));
      return;
    }
    redeem.mutate(
      { key, team_id: teamId },
      {
        onSuccess: () => {
          toast.success(t('wallet.redeemSuccess'));
          setRedeemKeyInput('');
          walletQuery.refetch();
          txQuery.refetch();
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const w = walletQuery.data;
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        <div className="rounded border p-3">
          <div className="text-xs text-muted-foreground">{t('team.balance')}</div>
          <div className="text-xl font-semibold">${w ? w.balance.toFixed(2) : '-'}</div>
        </div>
        <div className="rounded border p-3">
          <div className="text-xs text-muted-foreground">{t('team.frozen')}</div>
          <div className="text-xl font-semibold">${w ? w.frozen.toFixed(2) : '-'}</div>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Input
          value={redeemKeyInput}
          onChange={(e) => setRedeemKeyInput(e.target.value)}
          placeholder={t('wallet.keyInput')}
          className="flex-1"
          onKeyDown={(e) => e.key === 'Enter' && handleRedeem()}
        />
        <Button onClick={handleRedeem} disabled={redeem.isPending}>
          {t('wallet.redeemBtn')}
        </Button>
      </div>
      <div className="space-y-1">
        <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('team.members')} — {t('team.tabWallet')}
        </div>
        {txQuery.isLoading ? (
          <div className="p-2 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : (txQuery.data?.items ?? []).length === 0 ? (
          <EmptyState message={t('team.noTx')} />
        ) : (
          (txQuery.data?.items ?? []).map((tx) => (
            <div key={tx.id} className="flex justify-between rounded border px-3 py-2 text-sm">
              <div>
                <div className="text-xs">{tx.tx_type}</div>
                <div className="text-xs text-muted-foreground">{tx.created_at}</div>
              </div>
              <span className={tx.amount >= 0 ? 'text-emerald-600' : 'text-destructive'}>
                {tx.amount >= 0 ? '+' : ''}
                {tx.amount}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

