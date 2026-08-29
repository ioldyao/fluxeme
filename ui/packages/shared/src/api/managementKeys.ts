import { api } from './client';
import type { ManagementApiKey } from '../types';

export interface CreateManagementApiKeyRequest {
  name?: string;
  expires_at?: string;
}

export interface CreateManagementApiKeyResponse {
  key: string;
  metadata: ManagementApiKey;
}

export function listManagementApiKeys(): Promise<ManagementApiKey[]> {
  return api<ManagementApiKey[]>('/admin/management-keys');
}

export function createManagementApiKey(
  data: CreateManagementApiKeyRequest,
): Promise<CreateManagementApiKeyResponse> {
  return api<CreateManagementApiKeyResponse>('/admin/management-keys', {
    method: 'POST',
    body: data,
  });
}

export function setManagementApiKeyEnabled(
  id: string,
  enabled: boolean,
): Promise<{ id: string; enabled: boolean }> {
  return api<{ id: string; enabled: boolean }>(
    `/admin/management-keys/${encodeURIComponent(id)}/enabled`,
    { method: 'PUT', body: { enabled } },
  );
}

export function deleteManagementApiKey(id: string): Promise<{ deleted: string }> {
  return api<{ deleted: string }>(`/admin/management-keys/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}
