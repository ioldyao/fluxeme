import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useAdminSkills,
  useCreateSkill,
  useUpdateSkill,
  useDeleteSkill,
  useSetSkillStatus,
  useSkillVersions,
  useUploadSkillArtifact,
  useSkillRuntimeStatuses,
  useSkillScopes,
  useAddSkillScope,
  useDeleteSkillScope,
} from '@fluxeme/shared/src/api/skills';
import type { SkillRow } from '@fluxeme/shared/src/api/skills';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Textarea } from '@fluxeme/shared/src/components/ui/textarea';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@fluxeme/shared/src/components/ui/dialog';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@fluxeme/shared/src/components/ui/select';
import { Plus, RefreshCw, Upload, Pencil, Trash2, ListTree, Check, Package, ShieldCheck, KeyRound } from 'lucide-react';
import { toast } from 'sonner';
import { formatTimestamp } from '@fluxeme/shared/src/lib/date';

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  draft: { label: 'skillHub.status.draft', cls: 'bg-muted text-muted-foreground' },
  reviewing: { label: 'skillHub.status.reviewing', cls: 'bg-amber-100 text-amber-700' },
  approved: { label: 'skillHub.status.approved', cls: 'bg-blue-100 text-blue-700' },
  published: { label: 'skillHub.status.published', cls: 'bg-emerald-100 text-emerald-700' },
};

const VIS_LABEL: Record<string, string> = {
  public: 'skillHub.visibility.public',
  internal: 'skillHub.visibility.internal',
  private: 'skillHub.visibility.private',
};

const STATUS_FLOW: Record<string, string[]> = {
  draft: ['reviewing'],
  reviewing: ['approved'],
  approved: ['published'],
  published: ['draft'],
};

const RUNTIME_BADGE: Record<string, { label: string; cls: string }> = {
  pending: { label: 'skillHub.runtime.pending', cls: 'bg-muted text-muted-foreground' },
  ready: { label: 'skillHub.runtime.ready', cls: 'bg-emerald-100 text-emerald-700' },
  failed: { label: 'skillHub.runtime.failed', cls: 'bg-red-100 text-red-700' },
  disabled: { label: 'skillHub.runtime.disabled', cls: 'bg-muted text-muted-foreground' },
};

export default function SkillHubAdmin() {
  const { t } = useTranslation();
  const { data: skills, isLoading, isError, refetch } = useAdminSkills();
  const createMutation = useCreateSkill();
  const updateMutation = useUpdateSkill();
  const deleteMutation = useDeleteSkill();
  const statusMutation = useSetSkillStatus();
  const uploadMutation = useUploadSkillArtifact();
  const { data: runtimeStatuses } = useSkillRuntimeStatuses();

  const [statusFilter, setStatusFilter] = useState('');
  const [scoping, setScoping] = useState<SkillRow | null>(null);

  const runtimeBySkill = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of runtimeStatuses ?? []) m.set(s.skill_id, s.state);
    return m;
  }, [runtimeStatuses]);
  const [editing, setEditing] = useState<SkillRow | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [uploading, setUploading] = useState<SkillRow | null>(null);
  const [versionsOf, setVersionsOf] = useState<SkillRow | null>(null);

  // create form
  const [fSlug, setFSlug] = useState('');
  const [fName, setFName] = useState('');
  const [fDesc, setFDesc] = useState('');
  const [fCategory, setFCategory] = useState('');
  const [fTags, setFTags] = useState('');
  const [fVisibility, setFVisibility] = useState('internal');

  // upload form
  const [uVersion, setUVersion] = useState('');
  const [uChangelog, setUChangelog] = useState('');
  const [uFile, setUFile] = useState<File | null>(null);

  const filtered = useMemo(() => {
    if (!skills) return [];
    return statusFilter ? skills.filter((s) => s.status === statusFilter) : skills;
  }, [skills, statusFilter]);

  const openCreate = () => {
    setFSlug(''); setFName(''); setFDesc(''); setFCategory(''); setFTags(''); setFVisibility('internal');
    setCreateOpen(true);
  };

  const submitCreate = () => {
    if (!fSlug.trim() || !fName.trim()) {
      toast.error(t('err.loadFailed'));
      return;
    }
    createMutation.mutate(
      {
        slug: fSlug.trim(),
        name: fName.trim(),
        description: fDesc,
        category: fCategory,
        tags: fTags.split(',').map((x) => x.trim()).filter(Boolean),
        visibility: fVisibility as 'public' | 'internal' | 'private',
      },
      {
        onSuccess: () => { setCreateOpen(false); toast.success(t('common.add')); },
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  const submitUpdate = () => {
    if (!editing) return;
    updateMutation.mutate(
      {
        id: editing.id,
        data: {
          name: fName.trim(),
          description: fDesc,
          category: fCategory,
          tags: fTags.split(',').map((x) => x.trim()).filter(Boolean),
          visibility: fVisibility as 'public' | 'internal' | 'private',
        },
      },
      {
        onSuccess: () => { setEditing(null); toast.success('OK'); },
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  const openEdit = (s: SkillRow) => {
    setFName(s.name); setFDesc(s.description); setFCategory(s.category);
    setFTags(s.tags.join(',')); setFVisibility(s.visibility);
    setEditing(s);
  };

  const submitUpload = () => {
    if (!uploading || !uVersion.trim() || !uFile) return;
    uploadMutation.mutate(
      { skillId: uploading.id, version: uVersion.trim(), changelog: uChangelog, file: uFile },
      {
        onSuccess: () => {
          setUploading(null); setUVersion(''); setUChangelog(''); setUFile(null);
          toast.success('OK');
        },
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  const advanceStatus = (s: SkillRow, next: string) => {
    statusMutation.mutate(
      { id: s.id, status: next },
      {
        onSuccess: () => toast.success('OK'),
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  const confirmDelete = (s: SkillRow) => {
    if (window.confirm(`${t('skillHub.confirmDelete')}${s.name}${t('confirm.suffix')}`)) {
      deleteMutation.mutate(s.id, {
        onSuccess: () => toast.success('OK'),
        onError: (e: Error) => toast.error(e.message),
      });
    }
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('nav.skillHubAdmin')}
        description={t('skillHub.manageSubtitle')}
        actions={
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => refetch()}>
              <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
            </Button>
            <Button size="sm" onClick={openCreate}>
              <Plus className="size-4 mr-1" />{t('skillHub.create')}
            </Button>
          </div>
        }
      />

      {/* 状态筛选 */}
      <div className="flex items-center gap-2">
        <Select value={statusFilter} onValueChange={setStatusFilter}>
          <SelectTrigger className="w-44 h-9">
            <SelectValue placeholder={t('skillHub.status')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="">All</SelectItem>
            <SelectItem value="draft">{t('skillHub.status.draft')}</SelectItem>
            <SelectItem value="reviewing">{t('skillHub.status.reviewing')}</SelectItem>
            <SelectItem value="approved">{t('skillHub.status.approved')}</SelectItem>
            <SelectItem value="published">{t('skillHub.status.published')}</SelectItem>
          </SelectContent>
        </Select>
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
          ) : filtered.length === 0 ? (
            <EmptyState message={t('skillHub.empty')} />
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground">
                    <th className="text-left py-3 px-4">{t('skillHub.name')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.slug')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.category')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.status')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.runtimeStatus')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.version')}</th>
                    <th className="text-left py-3 px-4">{t('skillHub.updatedAt')}</th>
                    <th className="text-right py-3 px-4">{t('skillHub.actions')}</th>
                  </tr>
                </thead>
                <tbody>
                  {filtered.map((s) => {
                    const badge = STATUS_BADGE[s.status] ?? STATUS_BADGE.draft;
                    const next = (STATUS_FLOW[s.status] ?? [])[0];
                    return (
                      <tr key={s.id} className="border-b last:border-0 hover:bg-muted/50">
                        <td className="py-3 px-4">
                          <div className="font-medium">{s.name}</div>
                          <div className="text-xs text-muted-foreground">{t(VIS_LABEL[s.visibility] ?? 'skillHub.visibility.internal')}</div>
                        </td>
                        <td className="py-3 px-4 font-mono text-xs">{s.slug}</td>
                        <td className="py-3 px-4"><Badge variant="outline">{s.category}</Badge></td>
                        <td className="py-3 px-4"><Badge className={badge.cls}>{t(badge.label)}</Badge></td>
                        <td className="py-3 px-4">
                          <Badge className={RUNTIME_BADGE[runtimeBySkill.get(s.id) ?? 'pending']?.cls}>
                            {t(RUNTIME_BADGE[runtimeBySkill.get(s.id) ?? 'pending']?.label)}
                          </Badge>
                        </td>
                        <td className="py-3 px-4 font-mono text-xs">{s.version}</td>
                        <td className="py-3 px-4 text-xs text-muted-foreground">{formatTimestamp(s.updated_at)}</td>
                        <td className="py-3 px-4">
                          <div className="flex items-center justify-end gap-1">
                            {next && (
                              <Button size="sm" variant="outline" disabled={statusMutation.isPending} onClick={() => advanceStatus(s, next)}>
                                <Check className="size-3.5 mr-1" />
                                {t(`skillHub.status.${next}`)}
                              </Button>
                            )}
                            <Button size="icon" variant="ghost" title={t('skillHub.scope')} onClick={() => setScoping(s)}>
                              <ShieldCheck className="size-4" />
                            </Button>
                            <Button size="icon" variant="ghost" title={t('skillHub.upload')} onClick={() => { setUploading(s); setUVersion(''); setUChangelog(''); setUFile(null); }}>
                              <Upload className="size-4" />
                            </Button>
                            <Button size="icon" variant="ghost" title={t('skillHub.versions')} onClick={() => setVersionsOf(s)}>
                              <ListTree className="size-4" />
                            </Button>
                            <Button size="icon" variant="ghost" title={t('skillHub.edit')} onClick={() => openEdit(s)}>
                              <Pencil className="size-4" />
                            </Button>
                            <Button size="icon" variant="ghost" className="text-destructive" title={t('skillHub.delete')} onClick={() => confirmDelete(s)}>
                              <Trash2 className="size-4" />
                            </Button>
                          </div>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>

      {/* 新建 */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t('skillHub.create')}</DialogTitle></DialogHeader>
          <SkillForm
            slug={fSlug} onSlug={setFSlug}
            name={fName} onName={setFName}
            desc={fDesc} onDesc={setFDesc}
            category={fCategory} onCategory={setFCategory}
            tags={fTags} onTags={setFTags}
            visibility={fVisibility} onVisibility={setFVisibility}
            showSlug
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>{t('common.cancel')}</Button>
            <Button onClick={submitCreate} disabled={createMutation.isPending}>{t('skillHub.create')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 编辑 */}
      <Dialog open={!!editing} onOpenChange={(o) => { if (!o) setEditing(null); }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t('skillHub.edit')}</DialogTitle></DialogHeader>
          <SkillForm
            name={fName} onName={setFName}
            desc={fDesc} onDesc={setFDesc}
            category={fCategory} onCategory={setFCategory}
            tags={fTags} onTags={setFTags}
            visibility={fVisibility} onVisibility={setFVisibility}
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditing(null)}>{t('common.cancel')}</Button>
            <Button onClick={submitUpdate} disabled={updateMutation.isPending}>{t('common.save')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 上传包 */}
      <Dialog open={!!uploading} onOpenChange={(o) => { if (!o) setUploading(null); }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Package className="size-4" />{t('skillHub.upload')} {uploading ? `· ${uploading.name}` : ''}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-3">
            <label className="block text-sm font-medium">{t('skillHub.uploadVersion')}</label>
            <Input value={uVersion} onChange={(e) => setUVersion(e.target.value)} placeholder="1.0.0" />
            <label className="block text-sm font-medium">{t('skillHub.uploadChangelog')}</label>
            <Textarea value={uChangelog} onChange={(e) => setUChangelog(e.target.value)} rows={3} />
            <label className="block text-sm font-medium">{t('skillHub.uploadFile')}</label>
            <Input type="file" accept=".zip" onChange={(e) => setUFile(e.target.files?.[0] ?? null)} />
            <p className="text-xs text-muted-foreground">{t('skillHub.uploadHint')}</p>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setUploading(null)}>{t('common.cancel')}</Button>
            <Button onClick={submitUpload} disabled={uploadMutation.isPending || !uVersion.trim() || !uFile}>
              <Upload className="size-4 mr-1" />{t('skillHub.upload')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 版本列表 */}
      <VersionsDialog skill={versionsOf} onClose={() => setVersionsOf(null)} />

      {/* 授权管理 */}
      <ScopeDialog skill={scoping} onClose={() => setScoping(null)} />
    </div>
  );
}

function SkillForm(props: {
  slug?: string; onSlug?: (v: string) => void; showSlug?: boolean;
  name: string; onName: (v: string) => void;
  desc: string; onDesc: (v: string) => void;
  category: string; onCategory: (v: string) => void;
  tags: string; onTags: (v: string) => void;
  visibility: string; onVisibility: (v: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      {props.showSlug && (
        <>
          <label className="block text-sm font-medium">{t('skillHub.slug')}</label>
          <Input value={props.slug} onChange={(e) => props.onSlug?.(e.target.value)} placeholder="my-skill" />
        </>
      )}
      <label className="block text-sm font-medium">{t('skillHub.name')}</label>
      <Input value={props.name} onChange={(e) => props.onName(e.target.value)} />
      <label className="block text-sm font-medium">{t('skillHub.description')}</label>
      <Textarea value={props.desc} onChange={(e) => props.onDesc(e.target.value)} rows={3} />
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-sm font-medium">{t('skillHub.category')}</label>
          <Input value={props.category} onChange={(e) => props.onCategory(e.target.value)} placeholder="general" />
        </div>
        <div>
          <label className="block text-sm font-medium">{t('skillHub.tags')}</label>
          <Input value={props.tags} onChange={(e) => props.onTags(e.target.value)} placeholder="web,search" />
        </div>
      </div>
      <label className="block text-sm font-medium">{t('skillHub.visibility')}</label>
      <Select value={props.visibility} onValueChange={props.onVisibility}>
        <SelectTrigger className="h-9">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="public">{t('skillHub.visibility.public')}</SelectItem>
          <SelectItem value="internal">{t('skillHub.visibility.internal')}</SelectItem>
          <SelectItem value="private">{t('skillHub.visibility.private')}</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}

function VersionsDialog({ skill, onClose }: { skill: SkillRow | null; onClose: () => void }) {
  const { t } = useTranslation();
  const { data: versions, isLoading } = useSkillVersions(skill?.id ?? null);
  const [preview, setPreview] = useState<string | null>(null);
  return (
    <Dialog open={!!skill} onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t('skillHub.versions')} · {skill?.name}</DialogTitle></DialogHeader>
        {isLoading ? (
          <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : !versions || versions.length === 0 ? (
          <EmptyState message={t('skillHub.empty')} />
        ) : (
          <div className="space-y-2">
            {versions.map((v) => (
              <div key={v.id} className="flex items-center justify-between gap-2 p-3 rounded-lg border">
                <div className="min-w-0">
                  <div className="font-mono text-sm font-medium">{v.version}</div>
                  {v.changelog && <div className="text-xs text-muted-foreground truncate">{v.changelog}</div>}
                  <div className="text-[11px] text-muted-foreground mt-0.5">{formatTimestamp(v.created_at)} · {(v.artifact_size / 1024).toFixed(1)} KB</div>
                </div>
                {v.source_markdown && (
                  <Button variant="outline" size="sm" onClick={() => setPreview(preview === v.id ? null : v.id)}>
                    {t('skillHub.skillmd')}
                  </Button>
                )}
              </div>
            ))}
            {preview && (
              <pre className="rounded-lg bg-muted/50 p-3 text-xs whitespace-pre-wrap max-h-64 overflow-y-auto">
                {versions?.find((v) => v.id === preview)?.source_markdown}
              </pre>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** 授权管理：让指定 API Key 可调用此技能数据面（skill:{slug}:invoke）。 */
function ScopeDialog({ skill, onClose }: { skill: SkillRow | null; onClose: () => void }) {
  const { t } = useTranslation();
  const slug = skill?.slug ?? null;
  const { data: scopes, isLoading } = useSkillScopes(slug);
  const addMutation = useAddSkillScope();
  const deleteMutation = useDeleteSkillScope();
  const [key, setKey] = useState('');

  const submit = () => {
    if (!slug || !key.trim()) return;
    addMutation.mutate(
      { slug, key: key.trim() },
      {
        onSuccess: () => { setKey(''); toast.success('OK'); },
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  return (
    <Dialog open={!!skill} onOpenChange={(o) => { if (!o) onClose(); }}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="size-4" />{t('skillHub.scopeManage')} · {skill?.name}
          </DialogTitle>
        </DialogHeader>
        <p className="text-xs text-muted-foreground">{t('skillHub.scopeHint').replace('{slug}', skill?.slug ?? '')}</p>

        {/* 已授权列表 */}
        {isLoading ? (
          <div className="p-4 text-center text-muted-foreground">{t('common.loading')}</div>
        ) : !scopes || scopes.length === 0 ? (
          <div className="p-4 text-center text-muted-foreground text-sm">{t('skillHub.scopeNone')}</div>
        ) : (
          <div className="space-y-2">
            {scopes.map((s) => (
              <div key={s.id} className="flex items-center justify-between gap-2 p-2.5 rounded-lg border">
                <div className="min-w-0">
                  <div className="font-mono text-xs truncate">{s.api_key_id}</div>
                  <div className="text-[11px] text-muted-foreground">{s.key_name || '—'} · {s.action}</div>
                </div>
                <Button size="icon" variant="ghost" className="text-destructive shrink-0" onClick={() => deleteMutation.mutate({ slug: skill!.slug, scopeId: s.id })}>
                  <Trash2 className="size-4" />
                </Button>
              </div>
            ))}
          </div>
        )}

        {/* 添加 */}
        <div className="flex items-center gap-2">
          <Input
            placeholder={t('skillHub.scopeKey')}
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') submit(); }}
          />
          <Button onClick={submit} disabled={addMutation.isPending || !key.trim()}>
            <ShieldCheck className="size-4 mr-1" />{t('skillHub.scopeAdd')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
