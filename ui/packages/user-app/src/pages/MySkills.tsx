import { useTranslation } from 'react-i18next';
import { useMySkills, skillDownloadUrl } from '@fluxeme/shared/src/api/skills';
import { PageHeader } from '@fluxeme/shared/src/components/PageHeader';
import { EmptyState } from '@fluxeme/shared/src/components/EmptyState';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import { Card, CardContent } from '@fluxeme/shared/src/components/ui/card';
import { Badge } from '@fluxeme/shared/src/components/ui/badge';
import { RefreshCw, Download, CheckCircle2 } from 'lucide-react';
import { formatTimestamp } from '@fluxeme/shared/src/lib/date';

export default function MySkills() {
  const { t } = useTranslation();
  const { data: skills, isLoading, isError, refetch } = useMySkills();

  return (
    <div className="space-y-4 animate-fade-in">
      <PageHeader
        title={t('nav.mySkills')}
        description={t('skillHub.subtitle')}
        actions={
          <Button variant="outline" size="sm" onClick={() => refetch()}>
            <RefreshCw className="size-4 mr-1" />{t('common.refresh')}
          </Button>
        }
      />

      {isLoading ? (
        <div className="p-8 text-center text-muted-foreground">{t('common.loading')}</div>
      ) : isError ? (
        <div className="flex items-center justify-center p-8">
          <div className="text-center">
            <p className="text-destructive mb-2">{t('err.loadFailed')}</p>
            <Button variant="outline" onClick={() => refetch()}>{t('common.refresh')}</Button>
          </div>
        </div>
      ) : !skills || skills.length === 0 ? (
        <EmptyState message={t('skillHub.myEmpty')} />
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {skills.map(({ install, skill }) => (
            <Card key={install.id}>
              <CardContent className="p-4">
                <div className="flex items-start justify-between gap-2 mb-1.5">
                  <div className="min-w-0">
                    <div className="font-semibold truncate">{skill.name}</div>
                    <div className="text-xs font-mono text-muted-foreground truncate">@{skill.slug}</div>
                  </div>
                  <CheckCircle2 className="size-4 text-chart-2" />
                </div>
                <p className="text-sm text-muted-foreground line-clamp-2 min-h-[2.5rem]">{skill.description || '—'}</p>
                <div className="flex items-center gap-2 mt-2 text-xs text-muted-foreground">
                  <Badge variant="outline">{skill.category}</Badge>
                  <span className="font-mono">v{install.version}</span>
                </div>
                <div className="flex items-center justify-between mt-3 pt-3 border-t text-xs text-muted-foreground">
                  <span>{t('skillHub.myInstalledAt')} {formatTimestamp(install.installed_at)}</span>
                  <Button variant="outline" size="sm" asChild>
                    <a href={skillDownloadUrl(skill.slug, install.version)} download>
                      <Download className="size-3.5 mr-1" />{t('skillHub.download')}
                    </a>
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
