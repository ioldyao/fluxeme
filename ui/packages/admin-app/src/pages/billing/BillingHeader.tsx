import type { BillingCopy, BillingMonthOption, BillingView } from './types';

type Option = { value: string; label: string };

type Props = {
  copy: BillingCopy;
  view: BillingView;
  onViewChange: (view: BillingView) => void;
  months: BillingMonthOption[];
  safeSel: number;
  onSelectMonth: (index: number) => void;
  modelOptions: Option[];
  selectedModel: string;
  onSelectModel: (value: string) => void;
  channelOptions: Option[];
  selectedChannel: string;
  onSelectChannel: (value: string) => void;
  searchTerm: string;
  onSearchTermChange: (value: string) => void;
};

const TAB_ITEMS: Array<{ view: BillingView; label: string }> = [
  { view: 'overview', label: '概览' },
  { view: 'users', label: '用户账单' },
  { view: 'teams', label: '团队账单' },
  { view: 'keys', label: 'API Key' },
  { view: 'requests', label: '请求明细' },
];

export function BillingHeader({
  copy,
  view,
  onViewChange,
  months,
  safeSel,
  onSelectMonth,
  modelOptions,
  selectedModel,
  onSelectModel,
  channelOptions,
  selectedChannel,
  onSelectChannel,
  searchTerm,
  onSearchTermChange,
}: Props) {
  const isDetailView =
    view === 'user-detail' ||
    view === 'user-team' ||
    view === 'team-detail' ||
    view === 'key-detail' ||
    view === 'request-detail';

  if (isDetailView) return null;

  return (
    <div className="space-y-[14px]">
      <nav className="flex w-max gap-[4px] rounded-[10px] bg-[#edf0f5] p-[4px]">
        {TAB_ITEMS.map((tab) => (
          <button
            key={tab.view}
            type="button"
            onClick={() => onViewChange(tab.view)}
            className={`rounded-[7px] border-0 px-[13px] py-[8px] text-[11px] ${
              view === tab.view
                ? 'bg-white font-[700] text-[#243046] shadow-[0_2px_7px_rgba(20,30,50,.07)]'
                : 'bg-transparent text-[#657185]'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      <div className="flex flex-wrap items-center gap-[8px] rounded-[11px] border border-[#e6ebf2] bg-white px-[12px] py-[10px]">
        <label className="sr-only">{copy.monthLabel}</label>
        <select
          value={String(safeSel)}
          onChange={(e) => onSelectMonth(Number(e.target.value))}
          className="h-[32px] rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-[9px] text-[11px] text-[#465265]"
        >
          {months.length > 0 ? months.map((month, i) => (
            <option key={month.raw} value={i}>{month.label}</option>
          )) : <option value="0">{copy.noMonthData}</option>}
        </select>

        <label className="sr-only">{copy.modelLabel}</label>
        <select
          value={selectedModel}
          onChange={(e) => onSelectModel(e.target.value)}
          className="h-[32px] rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-[9px] text-[11px] text-[#465265]"
        >
          <option value="all">{copy.allModels}</option>
          {modelOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>

        <label className="sr-only">{copy.channelLabel}</label>
        <select
          value={selectedChannel}
          onChange={(e) => onSelectChannel(e.target.value)}
          className="h-[32px] rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-[9px] text-[11px] text-[#465265]"
        >
          <option value="all">{copy.allChannels}</option>
          {channelOptions.map((opt) => (
            <option key={opt.value} value={opt.value}>{opt.label}</option>
          ))}
        </select>

        <div className="grow" />

        <input
          value={searchTerm}
          onChange={(e) => onSearchTermChange(e.target.value)}
          placeholder={copy.searchPlaceholder}
          className="h-[32px] min-w-[280px] rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-[9px] text-[11px] text-[#465265]"
        />
      </div>
    </div>
  );
}
