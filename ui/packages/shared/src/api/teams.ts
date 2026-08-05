import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type { ApiKey, CreateKeyReq, RoutingRule, Team, TeamMember, TeamRole } from '../types';

function invalidateTeamQueries(queryClient: ReturnType<typeof useQueryClient>) {
  return queryClient.invalidateQueries({ queryKey: ['teams'] });
}

// ── Self-service ───────────────────────────────────────────────────

export function useMyTeams(enabled = true) {
  return useQuery({
    queryKey: ['teams', 'my'],
    queryFn: () => api<Team[]>('/teams'),
    enabled,
  });
}

export function useTeamMembers(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'members', teamId],
    queryFn: () => api<TeamMember[]>(`/teams/${encodeURIComponent(teamId)}/members`),
    enabled: !!teamId && enabled,
  });
}

export function useTeamWallet(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'wallet', teamId],
    queryFn: () =>
      api<{ team_id: string; balance: number; frozen: number }>(
        `/teams/${encodeURIComponent(teamId)}/wallet`,
      ),
    enabled: !!teamId && enabled,
  });
}

// ── Admin team management ──────────────────────────────────────────

export interface AdminTeam extends Team {
  role: string;
}

export function useAdminTeams() {
  return useQuery({
    queryKey: ['teams', 'admin'],
    queryFn: () => api<AdminTeam[]>('/admin/teams'),
  });
}

export function useAdminTeamMembers(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'admin', 'members', teamId],
    queryFn: () => api<TeamMember[]>(`/admin/teams/${encodeURIComponent(teamId)}/members`),
    enabled: !!teamId && enabled,
  });
}

export function useCreateTeam() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, ownerId }: { name: string; ownerId: string }) =>
      api<Team>('/admin/teams', { method: 'POST', body: { name, owner_id: ownerId } }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useUpdateTeam() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, name }: { teamId: string; name: string }) =>
      api<{ updated: string }>(`/admin/teams/${encodeURIComponent(teamId)}`, {
        method: 'PUT',
        body: { name },
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useDeleteTeam() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (teamId: string) =>
      api<{ deleted: string }>(`/admin/teams/${encodeURIComponent(teamId)}`, { method: 'DELETE' }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useAdminAddTeamMember() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId, role }: { teamId: string; userId: string; role?: TeamRole }) =>
      api<TeamMember>(`/admin/teams/${encodeURIComponent(teamId)}/members`, {
        method: 'POST',
        body: { user_id: userId, role: role ?? 'member' },
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useAdminSetTeamMemberRole() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId, role }: { teamId: string; userId: string; role: TeamRole }) =>
      api<{ updated: boolean }>(
        `/admin/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`,
        { method: 'PUT', body: { role } },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useAdminRemoveTeamMember() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId }: { teamId: string; userId: string }) =>
      api<{ removed: boolean }>(
        `/admin/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

// ── Team API Keys ─────────────────────────────────────────────────

export interface TeamWalletTx {
  id: string;
  user_id: string;
  tx_type: string;
  amount: number;
  balance_before: number;
  balance_after: number;
  method: string;
  status: string;
  note: string;
  created_at: string;
}

export function useTeamApiKeys(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'keys', teamId],
    queryFn: () => api<ApiKey[]>(`/teams/${encodeURIComponent(teamId)}/keys`),
    enabled: !!teamId && enabled,
  });
}

export function useAdminTeamApiKeys(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'admin', 'keys', teamId],
    queryFn: () => api<ApiKey[]>(`/admin/teams/${encodeURIComponent(teamId)}/keys`),
    enabled: !!teamId && enabled,
  });
}

export function useCreateTeamApiKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, data }: { teamId: string; data: CreateKeyReq }) =>
      api<{ key: string; team_id: string; name: string; enabled: boolean }>(
        `/teams/${encodeURIComponent(teamId)}/keys`,
        { method: 'POST', body: data },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useAdminCreateTeamApiKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, data }: { teamId: string; data: CreateKeyReq }) =>
      api<{ key: string; team_id: string; name: string; enabled: boolean }>(
        `/admin/teams/${encodeURIComponent(teamId)}/keys`,
        { method: 'POST', body: data },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useDeleteTeamApiKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, keyVal }: { teamId: string; keyVal: string }) =>
      api<{ deleted: string }>(
        `/teams/${encodeURIComponent(teamId)}/keys/${encodeURIComponent(keyVal)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

// ── Team member management (user-side, team admin) ────────────────

export function useAddTeamMember() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId, role }: { teamId: string; userId: string; role?: TeamRole }) =>
      api<TeamMember>(`/teams/${encodeURIComponent(teamId)}/members`, {
        method: 'POST',
        body: { user_id: userId, role: role ?? 'member' },
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useRemoveTeamMember() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId }: { teamId: string; userId: string }) =>
      api<{ removed: boolean }>(
        `/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useSetTeamMemberRole() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, userId, role }: { teamId: string; userId: string; role: TeamRole }) =>
      api<{ updated: boolean }>(
        `/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`,
        { method: 'PUT', body: { role } },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useCreditMyTeamWallet() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, amount }: { teamId: string; amount: number }) =>
      api<{ credited: number }>(`/teams/${encodeURIComponent(teamId)}/wallet`, {
        method: 'POST',
        body: { amount },
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

// ── Team Wallet ───────────────────────────────────────────────────

export function useTeamWalletTransactions(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'wallet', 'tx', teamId],
    queryFn: () =>
      api<{ items: TeamWalletTx[]; total: number }>(
        `/teams/${encodeURIComponent(teamId)}/wallet/transactions`,
      ),
    enabled: !!teamId && enabled,
  });
}

export function useAdminTeamWallet(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'admin', 'wallet', teamId],
    queryFn: () =>
      api<{ team_id: string; balance: number; frozen: number }>(
        `/admin/teams/${encodeURIComponent(teamId)}/wallet`,
      ),
    enabled: !!teamId && enabled,
  });
}

export function useCreditTeamWallet() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, amount }: { teamId: string; amount: number }) =>
      api<{ credited: number }>(`/admin/teams/${encodeURIComponent(teamId)}/wallet`, {
        method: 'POST',
        body: { amount },
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

// ── Team Rules ────────────────────────────────────────────────────

export function useTeamRules(teamId: string, enabled = true) {
  return useQuery({
    queryKey: ['teams', 'rules', teamId],
    queryFn: () => api<RoutingRule[]>(`/teams/${encodeURIComponent(teamId)}/rules`),
    enabled: !!teamId && enabled,
  });
}

export function useCreateTeamRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, data }: { teamId: string; data: Partial<RoutingRule> }) =>
      api<RoutingRule>(`/teams/${encodeURIComponent(teamId)}/rules`, {
        method: 'POST',
        body: data,
      }),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}

export function useDeleteTeamRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ teamId, ruleId }: { teamId: string; ruleId: string }) =>
      api<{ deleted: string }>(
        `/teams/${encodeURIComponent(teamId)}/rules/${encodeURIComponent(ruleId)}`,
        { method: 'DELETE' },
      ),
    onSuccess: () => invalidateTeamQueries(qc),
  });
}
