import { useTranslation } from 'react-i18next';
import { Bell, HelpCircle } from 'lucide-react';
import { Card, CardContent, CardHeader } from '@/components/ui/card';

export function DashboardInfoSection() {
  const { t } = useTranslation();

  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
      <Card className="card-hover">
        <CardHeader>
          <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
            <Bell className="size-4" />
            {t('dash.announcements')}
          </h2>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-muted-foreground">{t('dash.announcementsSub')}</p>
          <p className="mt-3 text-sm text-muted-foreground">{t('dash.noAnnouncements')}</p>
        </CardContent>
      </Card>
      <Card className="card-hover">
        <CardHeader>
          <h2 className="flex items-center gap-2 text-base font-semibold leading-none">
            <HelpCircle className="size-4" />
            {t('dash.faq')}
          </h2>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-muted-foreground">{t('dash.faqSub')}</p>
          <p className="mt-3 text-sm text-muted-foreground">{t('dash.noFaq')}</p>
        </CardContent>
      </Card>
    </div>
  );
}
