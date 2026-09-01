import { useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  usePublishedSkill,
  usePublishedSkillVersions,
  skillInstallCommand,
  skillDownloadUrl,
} from '@fluxeme/shared/src/api/skills';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { ArrowLeft, Download, Terminal, Check } from 'lucide-react';
import { toast } from 'sonner';
import { formatTimestamp } from '@fluxeme/shared/src/lib/date';
import { SkillMarkdown } from '@fluxeme/shared/src/components/SkillMarkdown';

const STATUS_BADGE: Record<string, { label: string; cls: string }> = {
  draft: { label: 'skillHub.status.draft', cls: 'bg-muted text-muted-foreground' },
  reviewing: { label: 'skillHub.status.reviewing', cls: 'bg-amber-100 text-amber-700' },
  approved: { label: 'skillHub.status.approved', cls: 'bg-blue-100 text-blue-700' },
  published: { label: 'skillHub.status.published', cls: 'bg-emerald-100 text-emerald-700' },
};

type TabKey = 'skillmd' | 'versions';

export default function SkillDetail() {
  const { slug = '' } = useParams<{ slug: string }>();
  const { t } = useTranslation();
  const { data: skill, isLoading, isError } = usePublishedSkill(slug);
  const { data: versions } = usePublishedSkillVersions(slug);
  const [tab, setTab] = useState<TabKey>('skillmd');
  const [copied, setCopied] = useState(false);

  if (isLoading) {
    return <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>;
  }
  if (isError || !skill) {
    return (
      <div className="p-8 text-center">
        <p className="text-destructive mb-3">{t('err.loadFailed')}</p>
        <Button variant="outline" asChild><Link to="/skills">{t('common.back')}</Link></Button>
      </div>
    );
  }

  const statusBadge = STATUS_BADGE[skill.status] ?? STATUS_BADGE.draft;
  const copyCommand = () => {
    navigator.clipboard.writeText(skillInstallCommand(skill.slug)).then(() => {
      setCopied(true);
      toast.success(t('skillHub.copied'));
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className="space-y-4 animate-fade-in">
      {/* 面包屑 / 返回 */}
      <div className="flex items-center gap-2 text-sm">
        <Button variant="ghost" size="sm" asChild className="-ml-2">
          <Link to="/skills"><ArrowLeft className="size-4 mr-1" />{t('nav.skillHub')}</Link>
        </Button>
        <span className="text-muted-foreground">/</span>
        <span className="text-muted-foreground">{skill.category}</span>
        <span className="text-muted-foreground">/</span>
        <b>{skill.name}</b>
      </div>

      {/* Hero */}
      <Card>
        <CardContent className="p-6">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <h1 className="text-2xl font-bold tracking-tight">{skill.name}</h1>
                <Badge className={statusBadge.cls}>{t(statusBadge.label)}</Badge>
              </div>
              <div className="mt-1 font-mono text-sm text-muted-foreground">@{skill.slug}</div>
              <p className="mt-3 max-w-2xl text-sm text-muted-foreground leading-relaxed">
                {skill.description || '—'}
              </p>
              <div className="mt-3 flex flex-wrap gap-1.5">
                <Badge variant="outline">{skill.category}</Badge>
                {skill.tags.map((tag) => (
                  <Badge key={tag} variant="secondary" className="text-xs">{tag}</Badge>
                ))}
              </div>
            </div>
            <Button variant="outline" asChild>
              <a href={skillDownloadUrl(skill.slug)} download>
                <Download className="size-4 mr-1" />{t('skillHub.download')}
              </a>
            </Button>
          </div>

          {/* 信息行 */}
          <div className="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-3">
            {[
              { label: t('skillHub.version'), value: `v${skill.version}` },
              { label: t('skillHub.category'), value: skill.category },
              { label: t('skillHub.updatedAt'), value: formatTimestamp(skill.updated_at) },
            ].map((it) => (
              <div key={it.label} className="rounded-lg border bg-muted/30 p-3">
                <div className="text-[11px] text-muted-foreground">{it.label}</div>
                <div className="mt-1 text-sm font-semibold">{it.value}</div>
              </div>
            ))}
          </div>

          {/* 一键安装命令 */}
          <div className="mt-5">
            <div className="mb-2 text-sm font-semibold">{t('skillHub.installCommand')}</div>
            <div className="relative">
              <pre className="overflow-x-auto whitespace-pre-wrap rounded-lg bg-muted p-3 pr-12 font-mono text-xs">
                {skillInstallCommand(skill.slug)}
              </pre>
              <Button
                variant="ghost"
                size="icon"
                className="absolute right-2 top-2 size-8"
                onClick={copyCommand}
                title={t('skillHub.copy')}
              >
                {copied ? <Check className="size-4 text-emerald-600" /> : <Terminal className="size-4" />}
              </Button>
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground">{t('skillHub.installHint')}</p>
          </div>
        </CardContent>
      </Card>

      {/* Tabs */}
      <Card>
        <div className="flex gap-4 border-b px-4">
          {(
            [
              { key: 'skillmd' as TabKey, label: t('skillHub.skillmd') },
              { key: 'versions' as TabKey, label: t('skillHub.versions') },
            ]
          ).map((tb) => (
            <button
              key={tb.key}
              onClick={() => setTab(tb.key)}
              className={`h-11 border-b-2 px-2 text-sm font-medium transition-colors ${
                tab === tb.key
                  ? 'border-primary text-foreground'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              {tb.label}
            </button>
          ))}
        </div>

        <CardContent className="p-4">
          {tab === 'skillmd' ? (
            skill.source_markdown ? (
              <article className="prose prose-sm dark:prose-invert max-w-none rounded-lg border bg-muted/30 p-4">
                <SkillMarkdown content={skill.source_markdown} />
              </article>
            ) : (
              <div className="p-6 text-center text-muted-foreground">{t('skillHub.empty')}</div>
            )
          ) : (
            <div className="space-y-2">
              {!versions || versions.length === 0 ? (
                <div className="p-6 text-center text-muted-foreground">{t('skillHub.empty')}</div>
              ) : (
                versions.map((v) => (
                  <div key={v.id} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border p-3">
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-sm font-semibold">v{v.version}</span>
                        {v.version === skill.version && (
                          <Badge className="bg-emerald-100 text-emerald-700">{t('skillHub.status.published')}</Badge>
                        )}
                      </div>
                      {v.changelog && (
                        <div className="mt-1 text-xs text-muted-foreground">{v.changelog}</div>
                      )}
                      <div className="mt-1 text-[11px] text-muted-foreground">
                        {formatTimestamp(v.created_at)} · {(v.artifact_size / 1024).toFixed(1)} KB
                      </div>
                    </div>
                    <Button variant="outline" size="sm" asChild>
                      <a href={skillDownloadUrl(skill.slug, v.version)} download>
                        <Download className="size-3.5 mr-1" />{t('skillHub.download')}
                      </a>
                    </Button>
                  </div>
                ))
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
