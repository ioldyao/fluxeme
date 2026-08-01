import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { api } from './client';
import type {
  CreateUserReq,
  UpdateUserReq,
  User,
  UserDetail,
  UserStatus,
} from '@shared/types';

function invalidateUserQueries(queryClient: ReturnType<typeof useQueryClient>) {
  return queryClient.invalidateQueries({ queryKey: ['users'] });
}

export function useUsers(status: UserStatus = 'active', enabled = true) {
  return useQuery({
    queryKey: ['users', 'list', status],
    queryFn: () => api<User[]>(`/users?status=${encodeURIComponent(status)}`),
    enabled,
  });
}

export function useUser(id: string) {
  return useQuery({
    queryKey: ['users', 'detail', id],
    queryFn: () => api<UserDetail>(`/users/${encodeURIComponent(id)}`),
    enabled: !!id,
  });
}

export function useCreateUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateUserReq) => api<User>('/users', { method: 'POST', body: data }),
    onSuccess: () => invalidateUserQueries(queryClient),
  });
}

export function useUpdateUser(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (data: UpdateUserReq) =>
      api<User>(`/users/${encodeURIComponent(id)}`, { method: 'PUT', body: data }),
    onSuccess: () => invalidateUserQueries(queryClient),
  });
}

export function useSuspendUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<User>(`/users/${encodeURIComponent(id)}/suspend`, { method: 'POST' }),
    onSuccess: () => invalidateUserQueries(queryClient),
  });
}

export function useRestoreUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<User>(`/users/${encodeURIComponent(id)}/restore`, { method: 'POST' }),
    onSuccess: () => invalidateUserQueries(queryClient),
  });
}

export function usePermanentDeleteUser() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<{ deleted: string }>(`/users/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => invalidateUserQueries(queryClient),
  });
}
