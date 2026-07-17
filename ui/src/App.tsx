import { useEffect } from 'react';
import { AppRoutes } from '@/routes';
import { useAuth } from '@/store/auth';
import { useCurrency } from '@/store/currency';

/** Sync server-persisted currency from auth store to currency store on every mount. */
function CurrencySyncer() {
  const authCurrency = useAuth((s) => s.currency);
  const { currency, setCurrency } = useCurrency();

  useEffect(() => {
    if (authCurrency && authCurrency !== currency) {
      setCurrency(authCurrency as 'usd' | 'cny');
    }
  }, [authCurrency, currency, setCurrency]);

  return null;
}

export default function App() {
  return (
    <>
      <CurrencySyncer />
      <AppRoutes />
    </>
  );
}
