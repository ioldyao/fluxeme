import { useState } from 'react';
import RoutingFlow from './RoutingFlow';
import RoutingHistory from './RoutingHistory';
import FlowTowerContent from './FlowTowerContent';

type Tab = 'tower' | 'routing' | 'history';

const TABS: { key: Tab; label: string }[] = [
  { key: 'tower', label: '流控台' },
  { key: 'routing', label: '路由流量' },
  { key: 'history', label: '历史查询' },
];

export default function FlowControlTower() {
  const [tab, setTab] = useState<Tab>('tower');

  return (
    <div className="space-y-4 animate-fade-in">
      {/* Tab bar */}
      <div className="flex items-center gap-1 border-b pb-0">
        {TABS.map(t => (
          <button
            key={t.key}
            type="button"
            onClick={() => setTab(t.key)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors -mb-px ${
              tab === t.key
                ? 'border-foreground text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
          >
            {t.label}
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
