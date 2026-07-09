import { cn } from '@/lib/utils'

function Slider({
  value,
  onValueChange,
  min = 0,
  max = 100,
  step = 1,
  id,
  className,
}: {
  value: number
  onValueChange: (value: number) => void
  min?: number
  max?: number
  step?: number
  id?: string
  className?: string
}) {
  const percent = ((value - min) / (max - min)) * 100
  return (
    <div className={cn('relative flex h-5 w-full items-center', className)}>
      <div className="relative h-1.5 w-full rounded-full bg-muted">
        <div
          className="absolute h-full rounded-full bg-primary"
          style={{ width: `${percent}%` }}
        />
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onValueChange(Number(e.target.value))}
        className="absolute inset-0 h-5 w-full cursor-pointer opacity-0"
      />
      <div
        className="pointer-events-none absolute size-4 -translate-x-1/2 rounded-full border-2 border-primary bg-background shadow-sm"
        style={{ left: `${percent}%` }}
      />
    </div>
  )
}

export { Slider }
