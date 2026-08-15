import type { ReactNode } from 'react';

type Props = {
  avatar: string;
  avatarColor?: string;
  name: string;
  meta: string;
  extra?: ReactNode;
  monthSelect?: ReactNode;
};

export function EntityHead({ avatar, avatarColor, name, meta, extra, monthSelect }: Props) {
  return (
    <div className="flex items-center justify-between gap-[14px] rounded-[12px] border border-[#e6ebf2] bg-white px-[16px] py-[14px]">
      <div className="flex items-center gap-[11px]">
        <div
          className="grid h-[40px] w-[40px] place-items-center rounded-[10px] font-[760]"
          style={{ background: avatarColor || '#eef1ff', color: '#5268f6' }}
        >
          {avatar}
        </div>
        <div>
          <div className="text-[15px] font-[750] text-[#182033]">{name}</div>
          <div className="mt-[3px] text-[9px] text-[#778296]">{meta}</div>
        </div>
      </div>
      <div className="flex items-center gap-2">
        {extra}
        {monthSelect}
      </div>
    </div>
  );
}
