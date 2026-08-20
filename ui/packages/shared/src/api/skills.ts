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
  visibility: string;
  status: string;
  published_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface SkillVersionRow {
  id: string;
  skill_id: string;
  version: string;
  changelog: string | null;
  artifact_path: string | null;
  artifact_size: number;
  source_markdown: string | null;
  status: string;
  created_by: string;
  created_at: string;
}

export interface SkillInstallRow {
  id: string;
  skill_id: string;
  user_id: string;
  version: string;
  source: string;
  installed_at: string;
}

export interface InstalledSkill {
  install: SkillInstallRow;
  skill: SkillRow;
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
    mutationFn: ({ id, status }: { id: string; status: string }) =>
      api<SkillRow>(`/admin/skills/${encodeURIComponent(id)}/status`, {
        method: 'POST',
        body: { status },
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

export function useInstallSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ slug, version }: { slug: string; version?: string }) =>
      api<SkillInstallRow>(`/skills/${encodeURIComponent(slug)}/install`, {
        method: 'POST',
        body: version ? { version } : {},
      }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['my-skills'] }),
  });
}

export function useMySkills() {
  return useQuery({
    queryKey: ['my-skills'],
    queryFn: () => api<InstalledSkill[]>('/me/skills'),
  });
}

/** 下载/一键安装用的完整 URL（带登录 cookie 可直链）。 */
export function skillDownloadUrl(slug: string, version?: string) {
  const API_BASE = import.meta.env.VITE_API_BASE_URL ?? '';
  return `${API_BASE}/api/skills/${encodeURIComponent(slug)}/download${
    version ? `?version=${encodeURIComponent(version)}` : ''
  }`;
}

/** 一键安装命令（curl + unzip 到 ~/.claude/skills/<slug>）。 */
export function skillInstallCommand(slug: string) {
  const url = skillDownloadUrl(slug);
  return `mkdir -p ~/.claude/skills/${slug} && curl -sL "${url}" -o /tmp/${slug}.zip && unzip -o /tmp/${slug}.zip -d ~/.claude/skills/${slug} && rm /tmp/${slug}.zip`;
}

// ── 运行状态 & API Key Scope（阶段 2：Skill Runtime） ─────────────────────

export interface SkillRuntimeStatusRow {
  skill_id: string;
  slug: string;
  version: string;
  state: string; // pending / ready / failed / disabled
}

/** 技能级运行状态（Skill Runtime 聚合，10s 轮询）。 */
export function useSkillRuntimeStatuses() {
  return useQuery({
    queryKey: ['skill-runtime-status'],
    queryFn: () => api<SkillRuntimeStatusRow[]>('/skills/runtime-status'),
    refetchInterval: 10000,
  });
}

