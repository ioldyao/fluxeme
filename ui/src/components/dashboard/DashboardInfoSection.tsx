import { useTranslation } from 'react-i18next';
import { Bell, HelpCircle } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

export function DashboardInfoSection() {
  const { t } = useTranslation();

  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
      <Card className="card-hover">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Bell className="size-4" />
            {t('dash.announcements')}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-muted-foreground">{t('dash.announcementsSub')}</p>
          <p className="mt-3 text-sm text-muted-foreground">{t('dash.noAnnouncements')}</p>
        </CardContent>
      </Card>
      <Card className="card-hover">
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <HelpCircle className="size-4" />
            FAQ
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xs text-muted-foreground">{t('dash.faqSub')}</p>
          <p className="mt-3 text-sm text-muted-foreground">{t('dash.noFaq')}</p>
        </CardContent>
      </Card>
    </div>
  );
}
