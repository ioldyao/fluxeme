import { useEffect, useMemo, useRef, useState } from 'react';
import { Input } from './input';

export interface ComboboxProps {
  options: string[];
  value: string;
  onValueChange: (value: string) => void;
  placeholder?: string;
  className?: string;
}

/** Free-form input with an optional prefix-filtered suggestion list. */
export function Combobox({ options, value, onValueChange, placeholder, className }: ComboboxProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const filteredOptions = useMemo(() => {
    const needle = value.trim().toLowerCase();
    return [...new Set(options)]
      .filter((option) => !needle || option.toLowerCase().startsWith(needle))
      .slice(0, 50);
  }, [options, value]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    return () => document.removeEventListener('pointerdown', onPointerDown);
  }, []);

  return (
    <div ref={rootRef} className="relative">
      <Input
        value={value}
        placeholder={placeholder}
        className={className}
        onFocus={() => setOpen(true)}
        onChange={(event) => { onValueChange(event.target.value); setOpen(true); }}
      />
      {open && filteredOptions.length > 0 && (
        <div className="absolute z-50 mt-1 max-h-56 w-full overflow-auto rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md">
          {filteredOptions.map((option) => (
            <button
              key={option}
              type="button"
              className="block w-full truncate rounded-sm px-2 py-1.5 text-left hover:bg-accent hover:text-accent-foreground"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => { onValueChange(option); setOpen(false); }}
            >
              {option}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
