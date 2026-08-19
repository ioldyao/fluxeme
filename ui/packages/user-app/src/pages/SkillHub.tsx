import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  usePublishedSkills,
  useInstallSkill,
  useMySkills,
  useSkillRuntimeStatuses,
  skillInstallCommand,
  skillDownloadUrl,
} from '@fluxeme/shared/src/api/skills';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@fluxeme/shared/src/components/ui/dialog';
import { Search, RefreshCw, Download, Terminal, Check } from 'lucide-react';
import { toast } from 'sonner';
import type { SkillRow } from '@fluxeme/shared/src/api/skills';

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  draft: { label: 'skillHub.status.draft', cls: 'bg-muted text-muted-foreground' },
  reviewing: { label: 'skillHub.status.reviewing', cls: 'bg-amber-100 text-amber-700' },
  approved: { label: 'skillHub.status.approved', cls: 'bg-blue-100 text-blue-700' },
  published: { label: 'skillHub.status.published', cls: 'bg-emerald-100 text-emerald-700' },
};

const RUNTIME_BADGE: Record<string, { label: string; cls: string }> = {
  pending: { label: 'skillHub.runtime.pending', cls: 'bg-muted text-muted-foreground' },
  ready: { label: 'skillHub.runtime.ready', cls: 'bg-emerald-100 text-emerald-700' },
  failed: { label: 'skillHub.runtime.failed', cls: 'bg-red-100 text-red-700' },
  disabled: { label: 'skillHub.runtime.disabled', cls: 'bg-muted text-muted-foreground' },
};

export default function SkillHub() {
  const { t } = useTranslation();
  const { data: skills, isLoading, isError, refetch } = usePublishedSkills();
  const { data: mySkills } = useMySkills();
  const { data: runtimeStatuses } = useSkillRuntimeStatuses();
  const installMutation = useInstallSkill();
  const [query, setQuery] = useState('');

  const runtimeBySlug = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of runtimeStatuses ?? []) m.set(s.slug, s.state);
    return m;
  }, [runtimeStatuses]);
  const [category, setCategory] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillRow | null>(null);
  const [copied, setCopied] = useState(false);

  const installedSlugs = useMemo(
    () => new Set((mySkills ?? []).map((m) => m.skill.slug)),
    [mySkills]
  );

  const categories = useMemo(() => {
    if (!skills) return [];
    return [...new Set(skills.map((s) => s.category).filter(Boolean))].sort();
  }, [skills]);

  const filtered = useMemo(() => {
    if (!skills) return [];
    return skills.filter((s) => {
      if (category && s.category !== category) return false;
      if (!query) return true;
      const q = query.toLowerCase();
      return (
        s.name.toLowerCase().includes(q) ||
        s.slug.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.tags.some((tag) => tag.toLowerCase().includes(q))
      );
    });
  }, [skills, query, category]);

  const copyCommand = () => {
    if (!detail) return;
    navigator.clipboard.writeText(skillInstallCommand(detail.slug)).then(() => {
      setCopied(true);
      toast.success(t('skillHub.copied'));
      setTimeout(() => setCopied(false), 1500);
    });
  };

  const doInstall = () => {
    if (!detail) return;
    installMutation.mutate(
      { slug: detail.slug },
      {
        onSuccess: () => toast.success(t('skillHub.installed')),
        onError: (e: Error) => toast.error(e.message),
      }
    );
  };

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('nav.skillHub')}
        description={t('skillHub.subtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      {/* 筛选 */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative flex-1 min-w-[220px]">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-4 text-muted-foreground" />
          <Input
            className="pl-8"
            placeholder={t('skillHub.search')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        {categories.map((c) => (
          <button
            key={c}
            onClick={() => setCategory(category === c ? null : c)}
            className={`px-3 py-1.5 rounded-full text-sm font-medium transition-colors border ${
              category === c
                ? 'bg-brand text-white border-brand'
                : 'text-muted-foreground hover:bg-accent border-border'
            }`}
          >
            {c}
          </button>
        ))}
      </div>

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
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {filtered.map((s) => {
            const badge = STATUS_BADGE[s.status] ?? STATUS_BADGE.draft;
            return (
              <Card key={s.id} className="cursor-pointer hover:shadow-md transition-shadow" onClick={() => setDetail(s)}>
                <CardContent className="p-4">
                  <div className="flex items-start justify-between gap-2 mb-1.5">
                    <div className="min-w-0">
                      <div className="font-semibold truncate">{s.name}</div>
                      <div className="text-xs font-mono text-muted-foreground truncate">{s.slug}</div>
                    </div>
                    <Badge className={badge.cls}>{t(badge.label)}</Badge>
                  </div>
                  <p className="text-sm text-muted-foreground line-clamp-2 min-h-[2.5rem]">{s.description || '—'}</p>
                  <div className="flex items-center gap-2 mt-2 text-xs text-muted-foreground">
                    <Badge variant="outline">{s.category}</Badge>
                    <span className="font-mono">v{s.version}</span>
                    <Badge className={RUNTIME_BADGE[runtimeBySlug.get(s.slug) ?? 'pending']?.cls}>
                      {t(RUNTIME_BADGE[runtimeBySlug.get(s.slug) ?? 'pending']?.label)}
                    </Badge>
                    {installedSlugs.has(s.slug) && <Badge className="bg-emerald-100 text-emerald-700">{t('skillHub.installed')}</Badge>}
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}

      {/* 详情弹窗 */}
      <Dialog open={!!detail} onOpenChange={(o) => { if (!o) setDetail(null); }}>
        <DialogContent className="max-w-2xl max-h-[85vh] overflow-y-auto">
          {detail && (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2 flex-wrap">
                  {detail.name}
                  <span className="text-sm font-mono text-muted-foreground">@{detail.slug}</span>
                  <Badge className={STATUS_BADGE[detail.status]?.cls}>{t(STATUS_BADGE[detail.status]?.label ?? 'skillHub.status.draft')}</Badge>
                  <Badge className={RUNTIME_BADGE[runtimeBySlug.get(detail.slug) ?? 'pending']?.cls}>
                    {t(RUNTIME_BADGE[runtimeBySlug.get(detail.slug) ?? 'pending']?.label)}
                  </Badge>
                  <Badge variant="outline">v{detail.version}</Badge>
                </DialogTitle>
              </DialogHeader>

              <div className="text-sm text-muted-foreground">{detail.description || '—'}</div>

              {detail.tags.length > 0 && (
                <div className="flex flex-wrap gap-1.5">
                  {detail.tags.map((tag) => (
                    <Badge key={tag} variant="secondary" className="text-xs">{tag}</Badge>
                  ))}
                </div>
              )}

              {/* 一键安装命令 */}
              <div className="space-y-2">
                <div className="text-sm font-semibold">{t('skillHub.installCommand')}</div>
                <div className="relative">
                  <pre className="rounded-lg bg-muted p-3 pr-10 text-xs overflow-x-auto whitespace-pre-wrap font-mono">
                    {skillInstallCommand(detail.slug)}
                  </pre>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="absolute right-1 top-1 size-8"
                    onClick={copyCommand}
                    title={t('skillHub.copy')}
                  >
                    {copied ? <Check className="size-4 text-emerald-600" /> : <Terminal className="size-4" />}
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">{t('skillHub.installHint')}</p>
              </div>

              {/* SKILL.md 预览 */}
              {detail.source_markdown && (
                <div className="space-y-2">
                  <div className="text-sm font-semibold">{t('skillHub.skillmd')}</div>
                  <pre className="rounded-lg bg-muted/50 p-3 text-xs whitespace-pre-wrap max-h-64 overflow-y-auto">
                    {detail.source_markdown}
                  </pre>
                </div>
              )}

              <DialogFooter className="flex gap-2">
                <Button
                  variant="outline"
                  asChild
                  className="flex-1"
                >
                  <a href={skillDownloadUrl(detail.slug)} download>
                    <Download className="size-4 mr-1" />{t('skillHub.download')}
                  </a>
                </Button>
                <Button
                  className="flex-1"
                  disabled={installedSlugs.has(detail.slug) || installMutation.isPending}
                  onClick={doInstall}
                >
                  {installedSlugs.has(detail.slug) ? t('skillHub.installed') : t('skillHub.install')}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
