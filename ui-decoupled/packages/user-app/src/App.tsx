import { SessionBootstrapper } from '@shared/routes/SessionBootstrapper';
import { AppRoutes } from '@/routes';

export default function App() {
  return (
    <>
      <SessionBootstrapper />
      <AppRoutes />
    </>
  );
}
