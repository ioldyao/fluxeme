import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import RoutingFlow from './RoutingFlow';
import RoutingHistory from './RoutingHistory';
import FlowTowerContent from './FlowTowerContent';

type Tab = 'tower' | 'routing' | 'history';

const TAB_KEYS: { key: Tab; i18n: string }[] = [
  { key: 'tower', i18n: 'nav.flowControl' },
  { key: 'routing', i18n: 'nav.routingFlow' },
  { key: 'history', i18n: 'nav.routingHistory' },
];

export default function FlowControlTower() {
  const { t } = useTranslation();
  const [tab, setTab] = useState<Tab>('tower');

  return (
    <div className="space-y-4 animate-fade-in">
      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b pb-0">
        {TAB_KEYS.map(tk => (
          <button
            key={tk.key}
            type="button"
            onClick={() => setTab(tk.key)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors -mb-px ${
              tab === tk.key
                ? 'border-foreground text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
          >
            {t(tk.i18n)}
          </button>
        ))}
      </div>

      {/* Tab content */}
      <div>
        {tab === 'tower' && <FlowTowerContent />}
        {tab === 'routing' && <RoutingFlow />}
        {tab === 'history' && <RoutingHistory />}
      </div>
    </div>
  );
}
