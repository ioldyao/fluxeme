import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { useActiveBillingGroups } from '@fluxeme/shared/src/api/apiKeys';
import {
  useMyTeams,
  useTeamMembers,
  useTeamWallet,
  useTeamApiKeys,
  useCreateTeamApiKey,
  useDeleteTeamApiKey,
  useTeamWalletTransactions,
  useTeamRules,
  useCreateTeamRule,
  useDeleteTeamRule,
  useAddTeamMember,
  useRemoveTeamMember,
  useSetTeamMemberRole,
} from '@fluxeme/shared/src/api/teams';
import { useRedeemKey } from '@fluxeme/shared/src/api/wallet';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { CopyButton } from '@fluxeme/shared/src/components/CopyButton';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@fluxeme/shared/src/components/ui/tabs';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@fluxeme/shared/src/components/ui/select';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@fluxeme/shared/src/components/ui/dialog';
import { Check, Plus, Trash2, UserPlus } from 'lucide-react';
import { toast } from 'sonner';
import type { CreateKeyReq, TeamRole } from '@fluxeme/shared/src/types';

export default function MyTeams() {
  const { t } = useTranslation();
  const { teams, activeTeamId, setActiveTeam } = useAuth();
  const myTeamsQuery = useMyTeams();
  const resolvedTeams = myTeamsQuery.data ?? teams;

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader title={t('nav.myTeams')} description={t('team.subtitle')} />

      {resolvedTeams.length === 0 ? (
        <Card>
          <CardContent className="p-8">
            <EmptyState message={t('team.noTeams')} />
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-3">
          <button
            onClick={() => setActiveTeam(null)}
            className={`flex w-full items-center justify-between rounded-lg border px-4 py-3 text-left hover:bg-muted/50 ${
              activeTeamId === null ? 'border-brand' : ''
            }`}
          >
            <div>
              <div className="font-medium">{t('team.personal')}</div>
              <div className="text-xs text-muted-foreground">{t('team.personal')}</div>
            </div>
            {activeTeamId === null && <Check className="size-4 text-brand" />}
          </button>

          {resolvedTeams.map((team) => (
            <TeamCard
              key={team.id}
              teamId={team.id}
              name={team.name}
              active={activeTeamId === team.id}
              onActivate={() => setActiveTeam(team.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function TeamCard({
  teamId,
  name,
  active,
  onActivate,
}: {
  teamId: string;
  name: string;
  active: boolean;
  onActivate: () => void;
}) {
  const { t } = useTranslation();
  const membersQuery = useTeamMembers(teamId, active);
  const walletQuery = useTeamWallet(teamId, active);

  return (
    <Card className={active ? 'border-brand' : ''}>
      <CardContent className="p-4">
        <button onClick={onActivate} className="flex w-full items-center justify-between text-left">
          <div>
            <div className="font-medium">{name}</div>
            <div className="text-xs text-muted-foreground">
              {t('team.members')}: {membersQuery.data?.length ?? '-'}
              {walletQuery.data
                ? ` · ${t('team.balance')}: $${walletQuery.data.balance.toFixed(2)}`
                : ''}
            </div>
          </div>
          {active && <Check className="size-4 text-brand" />}
        </button>
        {active && (
          <div className="mt-4">
            <TeamResources teamId={teamId} />
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function TeamResources({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const [tab, setTab] = useState('members');

  return (
    <Tabs value={tab} onValueChange={setTab}>
      <TabsList>
        <TabsTrigger value="members">{t('team.tabMembers')}</TabsTrigger>
        <TabsTrigger value="wallet">{t('team.tabWallet')}</TabsTrigger>
        <TabsTrigger value="keys">{t('team.tabKeys')}</TabsTrigger>
        <TabsTrigger value="rules">{t('team.tabRules')}</TabsTrigger>
      </TabsList>
      <div className="mt-3">
        <TabsContent value="members" className="mt-0">
          <TeamMembersView teamId={teamId} />
        </TabsContent>
        <TabsContent value="wallet" className="mt-0">
          <TeamWalletView teamId={teamId} />
        </TabsContent>
        <TabsContent value="keys" className="mt-0">
          <TeamKeysView teamId={teamId} />
        </TabsContent>
        <TabsContent value="rules" className="mt-0">
          <TeamRulesView teamId={teamId} />
        </TabsContent>
      </div>
    </Tabs>
  );
}

function TeamMembersView({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const { userId } = useAuth();
  const membersQuery = useTeamMembers(teamId);
  const addMember = useAddTeamMember();
  const removeMember = useRemoveTeamMember();
  const setRole = useSetTeamMemberRole();
  const [showAdd, setShowAdd] = useState(false);
  const [newId, setNewId] = useState('');
  const [newRole, setNewRole] = useState<TeamRole>('member');

  const members = membersQuery.data ?? [];
  const myRole = members.find((m) => m.user_id === userId)?.role;
  const canManage = myRole === 'owner' || myRole === 'admin';

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

  return (
    <div className="space-y-2">
      {canManage && (
        <Button size="sm" onClick={() => setShowAdd(true)}>
          <UserPlus className="mr-1 size-4" />
          {t('team.addMember')}
        </Button>
      )}
      {membersQuery.isLoading ? (
        <div className="p-2 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : members.length === 0 ? (
        <EmptyState message={t('team.emptyMembers')} />
      ) : (
        members.map((m) => (
          <div key={m.user_id} className="flex items-center justify-between rounded border px-3 py-2">
            <span className="font-mono text-sm">{m.user_id}</span>
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">{m.role}</span>
              {canManage && m.role !== 'owner' && (
                <>
                  <Select
                    value={m.role}
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
                </>
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

function TeamWalletView({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const { userId } = useAuth();
  const membersQuery = useTeamMembers(teamId);
  const walletQuery = useTeamWallet(teamId);
  const txQuery = useTeamWalletTransactions(teamId);
  const redeem = useRedeemKey();
  const w = walletQuery.data;
  const myRole = membersQuery.data?.find((m) => m.user_id === userId)?.role;
  const canManage = myRole === 'owner' || myRole === 'admin';
  const [redeemKeyInput, setRedeemKeyInput] = useState('');

  const handleRedeem = () => {
    const key = redeemKeyInput.trim();
    if (!key) {
      toast.error(t('common.required'));
      return;
    }
    redeem.mutate(
      key,
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

  return (
    <div className="space-y-3">
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
      {canManage && (
        <div className="flex items-center gap-2">
          <Input
            value={redeemKeyInput}
            onChange={(e) => setRedeemKeyInput(e.target.value)}
            placeholder={t('wallet.redeemKeyPlaceholder')}
            className="flex-1"
            onKeyDown={(e) => e.key === 'Enter' && handleRedeem()}
          />
          <Button onClick={handleRedeem} disabled={redeem.isPending}>
            {t('wallet.redeemKeyBtn')}
          </Button>
        </div>
      )}
      <div className="space-y-1">
        <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t('team.tabWallet')}
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
              <span className={tx.amount >= 0 ? 'text-chart-2' : 'text-destructive'}>
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

function TeamKeysView({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const { userId } = useAuth();
  const membersQuery = useTeamMembers(teamId);
  const keysQuery = useTeamApiKeys(teamId);
  const createKey = useCreateTeamApiKey();
  const deleteKey = useDeleteTeamApiKey();
  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState('');
  const [billingGroupId, setBillingGroupId] = useState('');
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const billingGroups = useActiveBillingGroups();

  const myRole = membersQuery.data?.find((m) => m.user_id === userId)?.role;
  const canManage = myRole === 'owner' || myRole === 'admin';

  const handleCreate = () => {
    createKey.mutate(
      { teamId, data: { name: name.trim() || undefined, billing_group_id: billingGroupId || null } as CreateKeyReq },
      {
        onSuccess: (res) => {
          setCreatedKey(res.key);
          setName('');
          setBillingGroupId('');
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const keys = keysQuery.data ?? [];
  return (
    <div className="space-y-2">
      {canManage && (
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus className="mr-1 size-4" />
          {t('team.createKey')}
        </Button>
      )}
      {keysQuery.isLoading ? (
        <div className="p-2 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : keys.length === 0 ? (
        <EmptyState message={t('team.noKeys')} />
      ) : (
        keys.map((k) => (
          <div key={k.key} className="flex items-center justify-between rounded border px-3 py-2">
            <div className="min-w-0">
              <div className="truncate font-mono text-xs">{k.key.substring(0, 24)}…</div>
              <div className="text-xs text-muted-foreground">{k.name || '-'}</div>
            </div>
            <div className="flex items-center gap-1">
              <CopyButton text={k.key} />
              {canManage && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    deleteKey.mutate(
                      { teamId, keyVal: k.key },
                      {
                        onSuccess: () => toast.success(t('toast.deleted')),
                        onError: (e) => toast.error(e.message),
                      },
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
      <Dialog open={showCreate || !!createdKey} onOpenChange={(open) => {
        if (!open) {
          setShowCreate(false);
          setCreatedKey(null);
          setBillingGroupId('');
        }
      }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{createdKey ? t('team.keyCreated') : t('team.createKey')}</DialogTitle>
          </DialogHeader>
          {createdKey ? (
            <div className="space-y-3">
              <div className="rounded bg-muted p-3 font-mono text-xs break-all">{createdKey}</div>
              <CopyButton text={createdKey} />
            </div>
          ) : (
            <>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={t('team.keyName')}
              />
              <select className="h-10 w-full rounded-md border bg-background px-3 text-sm" required value={billingGroupId} onChange={(event) => setBillingGroupId(event.target.value)} disabled={billingGroups.isLoading}>
                <option value="">{billingGroups.isLoading ? '正在加载计费分组…' : '请选择计费分组'}</option>
                {billingGroups.data?.map((group) => <option key={group.id} value={group.id}>{group.name} · {group.payment_mode === 'postpaid' ? '后付费' : '按量计费'}</option>)}
              </select>
              <div className="flex justify-end gap-3">
                <Button variant="outline" onClick={() => setShowCreate(false)}>
                  {t('common.cancel')}
                </Button>
                <Button onClick={handleCreate}>{t('common.save')}</Button>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function TeamRulesView({ teamId }: { teamId: string }) {
  const { t } = useTranslation();
  const { userId } = useAuth();
  const membersQuery = useTeamMembers(teamId);
  const rulesQuery = useTeamRules(teamId);
  const createRule = useCreateTeamRule();
  const deleteRule = useDeleteTeamRule();
  const [showCreate, setShowCreate] = useState(false);
  const [source, setSource] = useState('');
  const [target, setTarget] = useState('');

  const myRole = membersQuery.data?.find((m) => m.user_id === userId)?.role;
  const canManage = myRole === 'owner' || myRole === 'admin';

  const handleCreate = () => {
    if (!source.trim() || !target.trim()) {
      toast.error(t('common.required'));
      return;
    }
    createRule.mutate(
      { teamId, data: { source_model: source.trim(), target_model: target.trim() } },
      {
        onSuccess: () => {
          toast.success(t('toast.created'));
          setShowCreate(false);
          setSource('');
          setTarget('');
        },
        onError: (e) => toast.error(e.message),
      },
    );
  };

  const rules = rulesQuery.data ?? [];
  return (
    <div className="space-y-2">
      {canManage && (
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus className="mr-1 size-4" />
          {t('team.createRule')}
        </Button>
      )}
      {rulesQuery.isLoading ? (
        <div className="p-2 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : rules.length === 0 ? (
        <EmptyState message={t('team.noRules')} />
      ) : (
        rules.map((r) => (
          <div
            key={r.id}
            className="flex items-center justify-between rounded border px-3 py-2"
          >
            <div className="font-mono text-sm">
              {r.source_model} → {r.target_model}
            </div>
            {canManage && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() =>
                  deleteRule.mutate(
                    { teamId, ruleId: r.id },
                    {
                      onSuccess: () => toast.success(t('toast.deleted')),
                      onError: (e) => toast.error(e.message),
                    },
                  )
                }
              >
                <Trash2 className="size-3.5 text-destructive" />
              </Button>
            )}
          </div>
        ))
      )}
      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('team.createRule')}</DialogTitle>
          </DialogHeader>
          <Input
            value={source}
            onChange={(e) => setSource(e.target.value)}
            placeholder={t('team.sourceModel')}
          />
          <Input
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            placeholder={t('team.targetModel')}
          />
          <div className="flex justify-end gap-3">
            <Button variant="outline" onClick={() => setShowCreate(false)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={handleCreate}>{t('common.save')}</Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
