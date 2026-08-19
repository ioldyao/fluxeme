import * as React from "react";
import { CalendarIcon, Clock3, X } from "lucide-react";
import { format } from "date-fns";
import { useTranslation } from "react-i18next";

import { cn } from "../../lib/utils";
import { Button } from "./button";
import { Calendar } from "./calendar";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./select";

export interface DateTimePickerProps {
  value?: Date;
  onChange?: (value: Date | undefined) => void;
  placeholder?: string;
  disabled?: boolean;
  minuteStep?: number;
  clearable?: boolean;
  className?: string;
}

const HOURS = Array.from({ length: 24 }, (_, i) =>
  i.toString().padStart(2, "0")
);

function buildMinutes(step: number) {
  const s = Number.isFinite(step) && step > 0 ? Math.floor(step) : 1;
  const values: string[] = [];
  for (let i = 0; i < 60; i += s) {
    values.push(i.toString().padStart(2, "0"));
  }
  return values;
}

/** Theme-styled date + time picker built on the shadcn Calendar and Select. */
export function DateTimePicker({
  value,
  onChange,
  placeholder,
  disabled = false,
  minuteStep = 5,
  clearable = true,
  className,
}: DateTimePickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const minutes = React.useMemo(() => buildMinutes(minuteStep), [minuteStep]);

  const ensureBaseDate = React.useCallback(() => {
    if (value) return new Date(value);
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d;
  }, [value]);

  // Select a date but keep the picker open so the user can click through
  // multiple days (e.g. 17 then 19) before confirming.
  const handleDateChange = (date: Date | undefined) => {
    if (!date) return;

    const next = new Date(date);
    if (value) {
      next.setHours(value.getHours(), value.getMinutes(), 0, 0);
    } else {
      next.setHours(0, 0, 0, 0);
    }
    onChange?.(next);
  };

  const handleHourChange = (hour: string) => {
    const next = ensureBaseDate();
    next.setHours(Number(hour));
    next.setSeconds(0, 0);
    onChange?.(next);
  };

  const handleMinuteChange = (minute: string) => {
    const next = ensureBaseDate();
    next.setMinutes(Number(minute));
    next.setSeconds(0, 0);
    onChange?.(next);
  };

  const handleClear = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    onChange?.(undefined);
  };

  const hourValue = value ? String(value.getHours()).padStart(2, "0") : "00";
  const minuteValue = value ? String(value.getMinutes()).padStart(2, "0") : "00";

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          type="button"
          disabled={disabled}
          className={cn(
            "h-9 w-full justify-start gap-2 rounded-lg border bg-card px-3 text-left font-normal shadow-sm hover:bg-card/80",
            !value && "text-muted-foreground",
            className
          )}
        >
          <CalendarIcon className="size-4 shrink-0 text-muted-foreground" />
          <span className="flex-1 truncate">
            {value ? format(value, "yyyy-MM-dd HH:mm") : placeholder}
          </span>
          {value && clearable && !disabled && (
            <button
              type="button"
              aria-label={t('usage.clearRange')}
              onClick={handleClear}
              className="ml-auto inline-flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              <X className="size-3.5" />
            </button>
          )}
        </Button>
      </PopoverTrigger>

      <PopoverContent
        align="start"
        sideOffset={8}
        className="w-auto rounded-xl border bg-popover p-0 text-popover-foreground shadow-md ring-1 ring-foreground/10"
      >
        <Calendar mode="single" selected={value} onSelect={handleDateChange} />

        <div className="border-t px-4 py-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium">
            <Clock3 className="size-4 text-muted-foreground" />
            {t('usage.time')}
          </div>

          <div className="flex items-center gap-2">
            <Select value={hourValue} onValueChange={handleHourChange}>
              <SelectTrigger className="h-9 w-[88px]">
                <SelectValue placeholder="时" />
              </SelectTrigger>
              <SelectContent>
                {HOURS.map((hour) => (
                  <SelectItem key={hour} value={hour}>
                    {hour}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <span className="text-muted-foreground">:</span>

            <Select value={minuteValue} onValueChange={handleMinuteChange}>
              <SelectTrigger className="h-9 w-[88px]">
                <SelectValue placeholder="分" />
              </SelectTrigger>
              <SelectContent>
                {minutes.map((minute) => (
                  <SelectItem key={minute} value={minute}>
                    {minute}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <Button
              type="button"
              variant="ghost"
              className="ml-auto h-9 rounded-lg px-3"
              onClick={() => {
                const now = new Date();
                now.setSeconds(0, 0);
                onChange?.(now);
              }}
            >
              {t('usage.now')}
            </Button>
          </div>
        </div>

        <div className="flex items-center justify-between border-t px-4 py-3">
          <span className="text-xs text-muted-foreground">
            {value ? format(value, "yyyy-MM-dd HH:mm") : t('usage.pickTime')}
          </span>
          <Button
            type="button"
            className="h-9 rounded-lg px-4"
            disabled={!value}
            onClick={() => setOpen(false)}
          >
            {t('usage.confirm')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
