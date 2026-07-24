import { useTranslation } from 'react-i18next';
import { RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';

type DashboardToolbarProps = {
  days: number;
  onDaysChange: (days: number) => void;
  onRefresh: () => void;
};

const RANGE_OPTIONS = [7, 14, 30] as const;

export function DashboardToolbar({
  days,
  onDaysChange,
  onRefresh,
}: DashboardToolbarProps) {
  const { t } = useTranslation();

  return (
    <div className="rounded-xl border bg-card p-4 shadow-sm">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:gap-4">
          <fieldset className="flex items-center gap-2">
            <legend className="text-sm font-medium text-foreground">{t('dash.range')}</legend>
            <div className="flex flex-wrap items-center gap-2" role="group" aria-label={t('dash.range')}>
              {RANGE_OPTIONS.map((option) => {
                const isActive = option === days;
                return (
                  <button
                    key={option}
                    type="button"
                    aria-pressed={isActive}
                    onClick={() => onDaysChange(option)}
                    className={`rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${isActive ? 'bg-brand text-white shadow-sm' : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'}`}
                  >
                    {t('dash.rangePreset', { days: option })}
                  </button>
                );
              })}
            </div>
          </fieldset>
          <Button variant="outline" size="sm" onClick={onRefresh}>
            <RefreshCw className="mr-1 size-4" />
            {t('common.refresh')}
          </Button>
        </div>

        <div className="flex items-center gap-2 self-start lg:self-auto">
          <span className="text-sm text-muted-foreground">{t('dash.last24Hours')}</span>
        </div>
      </div>
    </div>
  );
}
