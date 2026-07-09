import { ChevronDown } from 'lucide-react'
import { cn } from '@/lib/utils'

type Option = { label: string; value: string }

function Select({
  value,
  onValueChange,
  options,
  className,
  id,
  'aria-label': ariaLabel,
}: {
  value: string
  onValueChange: (value: string) => void
  options: Option[]
  className?: string
  id?: string
  'aria-label'?: string
}) {
  return (
    <div className={cn('relative', className)}>
      <select
        id={id}
        aria-label={ariaLabel}
        value={value}
        onChange={(e) => onValueChange(e.target.value)}
        className={cn(
          'h-9 w-full appearance-none rounded-lg border border-input bg-background pl-3 pr-9 text-sm shadow-sm outline-none transition-colors',
          'focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40',
          'disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30',
        )}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      <ChevronDown className="pointer-events-none absolute right-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
    </div>
  )
}

export { Select }
