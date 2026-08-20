import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  usePublishedSkills,
  useMySkills,
  useSkillRuntimeStatuses,
} from '@fluxeme/shared/src/api/skills';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Input } from '@fluxeme/shared/src/components/ui/input';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { Search, RefreshCw } from 'lucide-react';

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
  const navigate = useNavigate();
  const { data: skills, isLoading, isError, refetch } = usePublishedSkills();
  const { data: mySkills } = useMySkills();
  const { data: runtimeStatuses } = useSkillRuntimeStatuses();
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState<string | null>(null);

  const runtimeBySlug = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of runtimeStatuses ?? []) m.set(s.slug, s.state);
    return m;
  }, [runtimeStatuses]);

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
              <Card
                key={s.id}
                className="cursor-pointer hover:shadow-md transition-shadow"
                onClick={() => navigate(`/skills/${encodeURIComponent(s.slug)}`)}
              >
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
    </div>
  );
}
