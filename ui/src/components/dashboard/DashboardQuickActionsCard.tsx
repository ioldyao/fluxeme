import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Braces, ChevronRight, Key, Receipt, Wallet } from 'lucide-react';
import { Card, CardContent, CardHeader } from '@/components/ui/card';

type QuickAction = {
  title: string;
  description: string;
  to: string;
  icon: ReactNode;
};

export function DashboardQuickActionsCard() {
  const { t } = useTranslation();

  const actions: QuickAction[] = [
    {
      title: t('nav.apiKeys'),
      description: t('apikey.subtitle'),
      to: '/api-keys',
      icon: <Key className="size-5" />,
    },
    {
      title: t('nav.usage'),
      description: t('usage.subtitle'),
      to: '/usage',
      icon: <Receipt className="size-5" />,
    },
    {
      title: t('nav.wallet'),
      description: t('wallet.subtitle'),
      to: '/wallet',
      icon: <Wallet className="size-5" />,
    },
    {
      title: t('nav.modelMarketplace'),
      description: t('market.subtitle'),
      to: '/models/marketplace',
      icon: <Braces className="size-5" />,
    },
  ];

  return (
    <Card className="card-hover">
      <CardHeader>
        <h2 className="text-base font-semibold leading-none">{t('dash.quickActions')}</h2>
        <p className="text-sm text-muted-foreground">{t('dash.quickActionsSub')}</p>
      </CardHeader>
      <CardContent className="space-y-3">
        {actions.map((action) => (
          <Link
            key={action.to}
            to={action.to}
            className="flex items-center gap-3 rounded-lg border bg-muted/20 p-3 transition-colors hover:border-brand/40 hover:bg-muted/40"
          >
            <div className="rounded-lg bg-brand/10 p-2 text-brand">{action.icon}</div>
            <div className="min-w-0 flex-1">
              <p className="font-medium text-foreground">{action.title}</p>
              <p className="truncate text-sm text-muted-foreground">{action.description}</p>
            </div>
            <ChevronRight className="size-4 text-muted-foreground" />
          </Link>
        ))}
      </CardContent>
    </Card>
  );
}
