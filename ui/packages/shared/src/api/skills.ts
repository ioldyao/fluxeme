import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { api } from './client';

// ── 类型（与 fluxeme-skillhub 后端行结构对应） ──────────────────────────

export interface SkillRow {
  id: string;
  slug: string;
  name: string;
  description: string;
  category: string;
  tags: string[];
  author_id: string;
  version: string;
  artifact_path: string | null;
  artifact_size: number;
  source_markdown: string | null;
  visibility: SkillVisibility;
  status: PackageStatus;
  published_at: string | null;
  created_at: string;
  updated_at: string;
  download_count: number;
  published_version_id: string | null;
}

export interface SkillVersionRow {
  id: string;
  skill_id: string;
  version: string;
  changelog: string | null;
  artifact_path: string | null;
  artifact_size: number;
  source_markdown: string | null;
  manifest_yaml: string | null;
  status: string;
  created_by: string;
  created_at: string;
}

export interface CreateSkillInput {
  slug: string;
  name: string;
  description?: string;
  category?: string;
  tags?: string[];
  visibility?: 'public' | 'internal' | 'private';
}

export interface UpdateSkillInput {
  name?: string;
  description?: string;
  category?: string;
  tags?: string[];
  visibility?: 'public' | 'internal' | 'private';
}

// ── 管理端 ──────────────────────────────────────────────────────────────

export function useAdminSkills(params?: { status?: string; visibility?: string }) {
  const q = new URLSearchParams();
  if (params?.status) q.set('status', params.status);
  if (params?.visibility) q.set('visibility', params.visibility);
  const qs = q.toString();
  return useQuery({
    queryKey: ['admin-skills', qs],
    queryFn: () => api<SkillRow[]>(`/admin/skills${qs ? `?${qs}` : ''}`),
  });
}

export function useAdminSkill(id: string | null) {
  return useQuery({
    queryKey: ['admin-skill', id],
    queryFn: () => api<SkillRow>(`/admin/skills/${encodeURIComponent(id!)}`),
    enabled: !!id,
  });
}

export function useCreateSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateSkillInput) =>
      api<SkillRow>('/admin/skills', { method: 'POST', body: data }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['admin-skills'] }),
  });
}

export function useUpdateSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateSkillInput }) =>
      api<SkillRow>(`/admin/skills/${encodeURIComponent(id)}`, { method: 'PATCH', body: data }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['admin-skills'] });
      qc.invalidateQueries({ queryKey: ['admin-skill'] });
    },
  });
}

export function useDeleteSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      api<{ ok: boolean }>(`/admin/skills/${encodeURIComponent(id)}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['admin-skills'] }),
  });
}

export function useSetSkillStatus() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, status, versionId }: { id: string; status: PackageStatus; versionId?: string }) =>
      api<SkillRow>(`/admin/skills/${encodeURIComponent(id)}/status`, {
        method: 'POST',
        body: { status, ...(versionId ? { version_id: versionId } : {}) },
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['admin-skills'] });
      qc.invalidateQueries({ queryKey: ['published-skills'] });
    },
  });
}

export function useSkillVersions(skillId: string | null) {
  return useQuery({
    queryKey: ['skill-versions', skillId],
    queryFn: () => api<SkillVersionRow[]>(`/admin/skills/${encodeURIComponent(skillId!)}/versions`),
    enabled: !!skillId,
  });
}

/** zip 上传走 multipart（client.ts 只支持 JSON，这里单独实现）。 */
export function useUploadSkillArtifact() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      skillId,
      version,
      changelog,
      file,
    }: {
      skillId: string;
      version: string;
      changelog?: string;
      file: File;
    }) => {
      const fd = new FormData();
      fd.append('version', version);
      if (changelog) fd.append('changelog', changelog);
      fd.append('file', file);
      const API_BASE = import.meta.env.VITE_API_BASE_URL ?? '';
      return fetch(`${API_BASE}/api/admin/skills/${encodeURIComponent(skillId)}/versions/upload`, {
        method: 'POST',
        credentials: 'include',
        body: fd,
      }).then(async (res) => {
        if (!res.ok) {
          const data = await res.json().catch(() => ({}));
          const message =
            typeof data.error === 'string'
              ? data.error
              : data.error?.message || data.message || 'Upload failed';
          throw new Error(message);
        }
        return res.json() as Promise<SkillVersionRow>;
      });
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['admin-skills'] });
      qc.invalidateQueries({ queryKey: ['skill-versions'] });
    },
  });
}

// ── 用户端 ──────────────────────────────────────────────────────────────

export function usePublishedSkills() {
  return useQuery({
    queryKey: ['published-skills'],
    queryFn: () => api<SkillRow[]>('/skills'),
  });
}

export function usePublishedSkill(slug: string | null) {
  return useQuery({
    queryKey: ['published-skill', slug],
    queryFn: () => api<SkillRow>(`/skills/${encodeURIComponent(slug!)}`),
    enabled: !!slug,
  });
}

/** 发布态技能的版本列表（详情页 Versions Tab）。 */
export function usePublishedSkillVersions(slug: string | null) {
  return useQuery({
    queryKey: ['published-skill-versions', slug],
    queryFn: () => api<SkillVersionRow[]>(`/skills/${encodeURIComponent(slug!)}/versions`),
    enabled: !!slug,
  });
}

function apiOrigin(): string {
  const configured = (import.meta.env.VITE_API_BASE_URL ?? '').replace(/\/+$/, '');
  const withoutApi = configured.replace(/\/api\/?$/, '');
  return withoutApi || (typeof window !== 'undefined' ? window.location.origin : 'http://localhost:8080');
}

/** 下载/一键安装用的完整 URL（浏览器同源时可携带登录 cookie）。 */
export function skillDownloadUrl(slug: string, version?: string) {
  return `${apiOrigin()}/api/skills/${encodeURIComponent(slug)}/download${
    version ? `?version=${encodeURIComponent(version)}` : ''
  }`;
}

/**
 * 生成 CLI 安装命令。命令不嵌入浏览器 session/API key；它仅适用于 public
 * 技能。curl 显式检查 HTTP 状态，避免把 JSON/HTML 错误响应当 ZIP 解压。
 */
export function skillInstallCommand(slug: string, version?: string) {
  const safeSlug = slug.replace(/[^A-Za-z0-9._-]/g, '_');
  const versionArg = version ? `?version=${encodeURIComponent(version)}` : '';
  const downloadUrl = `${apiOrigin()}/api/skills/${encodeURIComponent(slug)}/download${versionArg}`;
  return `set -eu; tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT; curl --fail-with-body -sSL "${downloadUrl}" -o "$tmp"; unzip -tq "$tmp" >/dev/null; mkdir -p "$HOME/.claude/skills/${safeSlug}"; unzip -o "$tmp" -d "$HOME/.claude/skills/${safeSlug}"`;
}

// ── 运行状态 & API Key Scope（阶段 2：Skill Runtime） ─────────────────────

export type SkillVisibility = 'public' | 'internal' | 'private';
export type PackageStatus = 'draft' | 'reviewing' | 'approved' | 'published' | 'disabled';
export type RuntimeState = 'pending' | 'ready' | 'failed' | 'disabled' | 'not_required';

export interface SkillRuntimeStatusRow {
  skill_id: string;
  slug: string;
  version: string;
  state: RuntimeState;
}

/** 技能级运行状态（Skill Runtime 聚合，10s 轮询）。 */
export function useSkillRuntimeStatuses() {
  return useQuery({
    queryKey: ['skill-runtime-status'],
    queryFn: () => api<SkillRuntimeStatusRow[]>('/skills/runtime-status'),
    refetchInterval: 10000,
  });
}

