import { useTranslation } from 'react-i18next';
import { PageHeader } from '@/components/PageHeader';

export default function Wallet() {
  const { t } = useTranslation();
  return (
    <div>
      <PageHeader title={t('wallet.title')} description={t('wallet.subtitle')} />
      <div className="p-8 text-center text-muted-foreground">{t('wallet.empty')}</div>
    </div>
  );
}
