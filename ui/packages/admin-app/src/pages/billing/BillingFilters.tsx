import { Input } from '@fluxeme/shared/src/components/ui/input';
import type { BillingCopy, BillingMonthOption } from './types';

type Option = {
  value: string;
  label: string;
};

type Props = {
  copy: BillingCopy;
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

export function BillingFilters({
  copy,
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
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-[12px] border border-[#e6ebf2] bg-white px-3 py-2">
      <span className="text-[10px] text-[#7c8798]">{copy.filterLabel}</span>

      <label className="sr-only">{copy.monthLabel}</label>
      <select
        value={String(safeSel)}
        onChange={(event) => onSelectMonth(Number(event.target.value))}
        className="h-8 rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-3 text-[11px] text-[#455166]"
      >
        {months.length > 0 ? months.map((month, index) => (
          <option key={month.raw} value={index}>{month.label}</option>
        )) : <option value="0">{copy.noMonthData}</option>}
      </select>

      <label className="sr-only">{copy.modelLabel}</label>
      <select
        value={selectedModel}
        onChange={(event) => onSelectModel(event.target.value)}
        className="h-8 rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-3 text-[11px] text-[#455166]"
      >
        <option value="all">{copy.allModels}</option>
        {modelOptions.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>

      <label className="sr-only">{copy.channelLabel}</label>
      <select
        value={selectedChannel}
        onChange={(event) => onSelectChannel(event.target.value)}
        className="h-8 rounded-[8px] border border-[#e6ebf2] bg-[#fbfcfe] px-3 text-[11px] text-[#455166]"
      >
        <option value="all">{copy.allChannels}</option>
        {channelOptions.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>

      <div className="grow" />

      <Input
        value={searchTerm}
        onChange={(event) => onSearchTermChange(event.target.value)}
        placeholder={copy.searchPlaceholder}
        className="h-8 min-w-[190px] rounded-[8px] border-[#e6ebf2] bg-[#fbfcfe] px-3 text-[11px] text-[#455166] md:w-[240px]"
      />
    </div>
  );
}
