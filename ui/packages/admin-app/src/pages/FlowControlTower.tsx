import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import FlowTowerContent from './FlowTowerContent';
import RoutingFlow from './RoutingFlow';
import RoutingHistory from './RoutingHistory';

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
      <div className="flex items-center gap-1 border-b pb-0" role="tablist" aria-label={t('flowControl.mainTabs')}>
        {TAB_KEYS.map((tk) => (
          <button
            key={tk.key}
            id={`flow-control-tab-${tk.key}`}
            type="button"
            role="tab"
            aria-selected={tab === tk.key}
            aria-controls={`flow-control-panel-${tk.key}`}
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

      <div>
        {tab === 'tower' && (
          <div id="flow-control-panel-tower" role="tabpanel" aria-labelledby="flow-control-tab-tower">
            <FlowTowerContent />
          </div>
        )}
        {tab === 'routing' && (
          <div id="flow-control-panel-routing" role="tabpanel" aria-labelledby="flow-control-tab-routing">
            <RoutingFlow />
          </div>
        )}
        {tab === 'history' && (
          <div id="flow-control-panel-history" role="tabpanel" aria-labelledby="flow-control-tab-history">
            <RoutingHistory />
          </div>
        )}
      </div>
    </div>
  );
}
