import * as React from "react"
import { CalendarIcon, ChevronLeft, ChevronRight, Clock3, X } from "lucide-react"
import { useTranslation } from "react-i18next"

import { cn } from "../../lib/utils"
import { Button } from "./button"
import { Popover, PopoverContent, PopoverTrigger } from "./popover"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./select"

export interface DateRangePickerProps {
  /** ISO datetime for the range start; empty when unset. */
  start: string
  /** ISO datetime for the range end; empty when unset. */
  end: string
  onStartChange: (value: string) => void
  onEndChange: (value: string) => void
  startPlaceholder?: string
  endPlaceholder?: string
  className?: string
}

const HOURS = Array.from({ length: 24 }, (_, i) => i)
const MINUTES = Array.from({ length: 12 }, (_, i) => i * 5)
const MONTHS_EN = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
]

const pad = (n: number) => String(n).padStart(2, "0")

const dayOnly = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
const sameDay = (a: Date | null | undefined, b: Date | null | undefined) =>
  !!a && !!b && dayOnly(a) === dayOnly(b)
const before = (a: Date, b: Date) => dayOnly(a) < dayOnly(b)
const after = (a: Date, b: Date) => dayOnly(a) > dayOnly(b)

function withTime(date: Date, source: Date | null | undefined) {
  return new Date(
    date.getFullYear(),
    date.getMonth(),
    date.getDate(),
    source ? source.getHours() : 0,
    source ? source.getMinutes() : 0,
    0,
    0
  )
}

function fmtDate(d: Date | null | undefined) {
  return d ? `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` : ""
}

function fmtDateTime(d: Date | null | undefined) {
  return d ? `${fmtDate(d)} ${pad(d.getHours())}:${pad(d.getMinutes())}` : ""
}

function parseIso(value: string): Date | null {
  if (!value) return null
  const d = new Date(value)
  return Number.isNaN(d.getTime()) ? null : d
}

type Handle = null | "start" | "end" | "new-end"

function DateBox({
  label,
  value,
  active,
  onClick,
}: {
  label: string
  value: string
  active: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-11 flex-col justify-center rounded-xl border bg-card px-3 text-left transition-colors",
        active
          ? "border-primary/40 ring-2 ring-primary/10"
          : "hover:border-border/80"
      )}
    >
      <span className="mb-1 text-[10px] leading-none text-muted-foreground">{label}</span>
      <span className="text-[13px] font-bold">{value || "—"}</span>
    </button>
  )
}

function TimeSelects({
  date,
  onChange,
}: {
  date: Date | null
  onChange: (value: string) => void
}) {
  const { t } = useTranslation()
  const hour = date ? date.getHours() : null
  const minute = date ? date.getMinutes() : null
  const commit = (h: number, m: number) => {
    const base = date ?? new Date()
    onChange(new Date(base.getFullYear(), base.getMonth(), base.getDate(), h, m, 0, 0).toISOString())
  }
  return (
    <div className="flex items-center gap-1.5">
      <Select value={hour != null ? pad(hour) : "00"} onValueChange={(v) => commit(Number(v), minute ?? 0)}>
        <SelectTrigger className="h-9 w-full" disabled={!date}>
          <SelectValue placeholder={t('usage.hour')} />
        </SelectTrigger>
        <SelectContent>
          {HOURS.map((h) => (
            <SelectItem key={h} value={pad(h)}>
              {pad(h)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <span className="text-muted-foreground">:</span>
      <Select value={minute != null ? pad(minute) : "00"} onValueChange={(v) => commit(hour ?? 0, Number(v))}>
        <SelectTrigger className="h-9 w-full" disabled={!date}>
          <SelectValue placeholder={t('usage.minute')} />
        </SelectTrigger>
        <SelectContent>
          {MINUTES.map((m) => (
            <SelectItem key={m} value={pad(m)}>
              {pad(m)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

/** Range date + time picker: pick start then end in one continuous calendar flow. */
export function DateRangePicker({
  start,
  end,
  onStartChange,
  onEndChange,
  startPlaceholder,
  endPlaceholder,
  className,
}: DateRangePickerProps) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = React.useState(false)
  const [activeHandle, setActiveHandle] = React.useState<Handle>(null)
  const [hoverDate, setHoverDate] = React.useState<Date | null>(null)

  const startDate = parseIso(start)
  const endDate = parseIso(end)

  const [view, setView] = React.useState(() => {
    const base = startDate ?? new Date()
    return { year: base.getFullYear(), month: base.getMonth() }
  })
  React.useEffect(() => {
    if (open && startDate) {
      setView({ year: startDate.getFullYear(), month: startDate.getMonth() })
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const handleOpenChange = (o: boolean) => {
    setOpen(o)
    if (!o) {
      setActiveHandle(null)
      setHoverDate(null)
    }
  }

  const selectDate = (date: Date) => {
    let nextStart = startDate
    let nextEnd = endDate
    let nextHandle: Handle = null

    // Click the current start/end endpoint to re-select only that endpoint.
    if (startDate && sameDay(date, startDate) && activeHandle !== "new-end") {
      setActiveHandle("start")
      setHoverDate(null)
      return
    }
    if (endDate && sameDay(date, endDate) && activeHandle !== "new-end") {
      setActiveHandle("end")
      setHoverDate(null)
      return
    }

    if (activeHandle === "start") {
      const ns = withTime(date, startDate)
      if (nextEnd && after(ns, nextEnd)) {
        const oldEnd = nextEnd
        nextStart = withTime(oldEnd, startDate)
        nextEnd = withTime(ns, endDate)
      } else {
        nextStart = ns
      }
    } else if (activeHandle === "end") {
      const ne = withTime(date, endDate)
      if (nextStart && before(ne, nextStart)) {
        const oldStart = nextStart
        nextStart = withTime(ne, startDate)
        nextEnd = withTime(oldStart, endDate)
      } else {
        nextEnd = ne
      }
    } else if (activeHandle === "new-end") {
      const candidate = withTime(date, endDate)
      if (startDate && before(candidate, startDate)) {
        const first = startDate
        nextStart = withTime(candidate, startDate)
        nextEnd = withTime(first, endDate)
      } else {
        nextEnd = candidate
      }
    } else {
      // No active handle: begin a brand-new range from this date.
      nextStart = withTime(date, startDate)
      nextEnd = null
      nextHandle = "new-end"
    }

    onStartChange(nextStart ? nextStart.toISOString() : "")
    onEndChange(nextEnd ? nextEnd.toISOString() : "")
    if (nextHandle !== null) setActiveHandle(nextHandle)
    setView({ year: date.getFullYear(), month: date.getMonth() })
    setHoverDate(null)
  }

  // ── Month grid (Monday-start, 6 weeks) ──
  const first = new Date(view.year, view.month, 1)
  const startDow = (first.getDay() + 6) % 7
  const daysInMonth = new Date(view.year, view.month + 1, 0).getDate()
  const prevDays = new Date(view.year, view.month, 0).getDate()

  const cells: { date: Date; outside: boolean }[] = []
  for (let i = 0; i < 42; i++) {
    let y = view.year
    let m = view.month
    let n: number
    let outside = false
    if (i < startDow) {
      n = prevDays - startDow + i + 1
      m--
      if (m < 0) {
        m = 11
        y--
      }
      outside = true
    } else if (i >= startDow + daysInMonth) {
      n = i - (startDow + daysInMonth) + 1
      m++
      if (m > 11) {
        m = 0
        y++
      }
      outside = true
    } else {
      n = i - startDow + 1
    }
    cells.push({ date: new Date(y, m, n), outside })
  }

  const preview = (() => {
    if (!hoverDate) return null
    if (activeHandle === "new-end" && startDate)
      return before(hoverDate, startDate) ? [hoverDate, startDate] : [startDate, hoverDate]
    if (activeHandle === "start" && endDate)
      return before(hoverDate, endDate) ? [hoverDate, endDate] : [endDate, hoverDate]
    if (activeHandle === "end" && startDate)
      return before(hoverDate, startDate) ? [hoverDate, startDate] : [startDate, hoverDate]
    return null
  })()

  const isZh = i18n.language?.startsWith("zh")
  const WEEKDAYS = isZh ? ["一", "二", "三", "四", "五", "六", "日"] : ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
  const monthTitle = isZh ? `${view.year}年${view.month + 1}月` : `${MONTHS_EN[view.month]} ${view.year}`
  const today = new Date()

  const triggerLabel =
    startDate && endDate
      ? `${fmtDateTime(startDate)} → ${fmtDateTime(endDate)}`
      : startDate
        ? `${fmtDateTime(startDate)} → ${t('usage.pickEndTime')}`
        : `${startPlaceholder ?? ""} → ${endPlaceholder ?? ""}`

  const prevMonth = () =>
    setView((v) => (v.month === 0 ? { year: v.year - 1, month: 11 } : { year: v.year, month: v.month - 1 }))
  const nextMonth = () =>
    setView((v) => (v.month === 11 ? { year: v.year + 1, month: 0 } : { year: v.year, month: v.month + 1 }))

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          className={cn(
            "h-9 w-full justify-start gap-2 rounded-lg border bg-card px-3 text-left font-normal shadow-sm hover:bg-card/80",
            !startDate && "text-muted-foreground",
            className
          )}
        >
          <CalendarIcon className="size-4 shrink-0 text-muted-foreground" />
          <span className="flex-1 truncate font-mono text-xs">{triggerLabel}</span>
          {(startDate || endDate) && (
            <button
              type="button"
              aria-label={t('usage.clearRange')}
              onClick={(e) => {
                e.stopPropagation()
                onStartChange("")
                onEndChange("")
              }}
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
        {/* Calendar */}
        <div className="w-[320px] px-4 pb-3 pt-4">
          <div className="mb-3 grid grid-cols-[40px_1fr_40px] items-center">
            <button
              type="button"
              aria-label="上个月"
              onClick={prevMonth}
              className="grid size-9 place-items-center rounded-[11px] text-muted-foreground transition-colors hover:bg-accent"
            >
              <ChevronLeft className="size-4" />
            </button>
            <div className="text-center text-[15px] font-bold tracking-tight">{monthTitle}</div>
            <button
              type="button"
              aria-label="下个月"
              onClick={nextMonth}
              className="grid size-9 place-items-center rounded-[11px] text-muted-foreground transition-colors hover:bg-accent"
            >
              <ChevronRight className="size-4" />
            </button>
          </div>

          <div className="grid grid-cols-7">
            {WEEKDAYS.map((w) => (
              <div key={w} className="grid h-8 place-items-center text-xs font-medium text-muted-foreground">
                {w}
              </div>
            ))}
          </div>

          <div className="grid grid-cols-7">
            {cells.map(({ date, outside }, i) => {
              const isStart = sameDay(date, startDate)
              const isEnd = sameDay(date, endDate)
              const isBetween = startDate && endDate && after(date, startDate) && before(date, endDate)
              const isActiveHandle = (isStart && activeHandle === "start") || (isEnd && activeHandle === "end")
              const inPreview = (() => {
                if (!preview) return false
                return (
                  sameDay(date, preview[0]) ||
                  sameDay(date, preview[1]) ||
                  (after(date, preview[0]) && before(date, preview[1]))
                )
              })()
              const previewBg = inPreview && !isStart && !isEnd && !isBetween
              return (
                <div key={i} className="relative grid h-[46px] place-items-center">
                  <div
                    className={cn(
                      "absolute inset-x-0 top-[5px] h-10",
                      isBetween && "bg-accent/70",
                      isStart && "bg-accent/70 rounded-l-[12px]",
                      isEnd && "bg-accent/70 rounded-r-[12px]",
                      isStart && isEnd && "rounded-[12px]",
                      previewBg && "bg-accent/40"
                    )}
                  />
                  <button
                    type="button"
                    onClick={() => selectDate(date)}
                    onMouseEnter={() => {
                      if (activeHandle) setHoverDate(date)
                    }}
                    onMouseLeave={() => {
                      if (activeHandle && hoverDate) setHoverDate(null)
                    }}
                    className={cn(
                      "relative z-10 grid size-10 place-items-center rounded-[13px] text-sm transition-colors",
                      isStart || isEnd
                        ? "bg-primary font-bold text-primary-foreground hover:bg-primary"
                        : "hover:bg-accent",
                      isActiveHandle && "ring-2 ring-primary/40 ring-offset-1 ring-offset-background",
                      outside && "text-muted-foreground/60",
                      sameDay(date, today) && !isStart && !isEnd && "shadow-[inset_0_0_0_1px_var(--border)]"
                    )}
                  >
                    {date.getDate()}
                  </button>
                </div>
              )
            })}
          </div>
        </div>

        {/* Selected range summary */}
        <div className="border-t bg-muted/20 px-4 py-3">
          <div className="mb-2 text-[11px] font-bold uppercase tracking-wider text-muted-foreground">
            {t('usage.selectedRange')}
          </div>
          <div className="grid grid-cols-[1fr_18px_1fr] items-center gap-2">
            <DateBox
              label={t('usage.start')}
              value={fmtDate(startDate)}
              active={activeHandle === "start"}
              onClick={() => {
                if (startDate) {
                  setActiveHandle("start")
                  setHoverDate(null)
                }
              }}
            />
            <div className="text-center text-muted-foreground">→</div>
            <DateBox
              label={t('usage.end')}
              value={fmtDate(endDate)}
              active={activeHandle === "end"}
              onClick={() => {
                if (endDate) {
                  setActiveHandle("end")
                  setHoverDate(null)
                }
              }}
            />
          </div>
        </div>

        {/* Time selects */}
        <div className="border-t px-4 py-3">
          <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-muted-foreground">
            <Clock3 className="size-3.5" />
            {t('usage.time')}
          </div>
          <div className="grid grid-cols-[1fr_12px_1fr] items-center gap-2">
            <TimeSelects date={startDate} onChange={onStartChange} />
            <div className="text-center text-muted-foreground">→</div>
            <TimeSelects date={endDate} onChange={onEndChange} />
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between gap-3 border-t px-4 py-3">
          <span className="min-w-0 truncate text-[11px] text-muted-foreground">
            {startDate && endDate ? `${fmtDate(startDate)} → ${fmtDate(endDate)}` : t('usage.pickRange')}
          </span>
          <Button type="button" size="sm" className="rounded-lg px-4" onClick={() => setOpen(false)}>
            {t('usage.confirm')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
