import { Menu, Moon, Sun, Search, Bell } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTheme } from '@/components/theme-provider'

export function Topbar({ onMenuClick }: { onMenuClick: () => void }) {
  const { theme, toggleTheme } = useTheme()

  return (
    <header className="sticky top-0 z-20 flex h-16 items-center gap-3 border-b border-border bg-background/80 px-4 backdrop-blur-md lg:px-6">
      <Button
        variant="ghost"
        size="icon"
        className="lg:hidden"
        onClick={onMenuClick}
        aria-label="打开菜单"
      >
        <Menu className="size-5" />
      </Button>

      <div className="relative hidden max-w-md flex-1 sm:block">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <input
          type="search"
          placeholder="搜索模型、文档或密钥…"
          className="h-9 w-full rounded-lg border border-input bg-muted/40 pl-9 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:bg-background focus-visible:ring-[3px] focus-visible:ring-ring/40"
        />
      </div>

      <div className="ml-auto flex items-center gap-1.5">
        <Button
          variant="ghost"
          size="icon"
          onClick={toggleTheme}
          aria-label={theme === 'dark' ? '切换到浅色模式' : '切换到暗色模式'}
        >
          {theme === 'dark' ? <Sun className="size-5" /> : <Moon className="size-5" />}
        </Button>
        <Button variant="ghost" size="icon" aria-label="通知" className="relative">
          <Bell className="size-5" />
          <span className="absolute right-2 top-2 size-1.5 rounded-full bg-primary" />
        </Button>
        <div className="ml-1.5 flex items-center gap-2 rounded-lg border border-border py-1 pl-1 pr-2.5">
          <div className="flex size-7 items-center justify-center rounded-md bg-primary/15 text-xs font-semibold text-primary">
            LZ
          </div>
          <div className="hidden flex-col leading-tight sm:flex">
            <span className="text-xs font-medium">李哲</span>
            <span className="text-[10px] text-muted-foreground">Pro 套餐</span>
          </div>
        </div>
      </div>
    </header>
  )
}
