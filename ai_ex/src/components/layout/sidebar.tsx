import { NavLink } from 'react-router-dom'
import {
  LayoutDashboard,
  Boxes,
  KeyRound,
  TerminalSquare,
  BookOpen,
  Zap,
  X,
} from 'lucide-react'
import { cn } from '@/lib/utils'

const nav = [
  { to: '/', label: '仪表盘', icon: LayoutDashboard, end: true },
  { to: '/models', label: '模型市场', icon: Boxes },
  { to: '/playground', label: 'Playground', icon: TerminalSquare },
  { to: '/keys', label: 'API 密钥', icon: KeyRound },
  { to: '/docs', label: '文档', icon: BookOpen },
]

export function Sidebar({
  open,
  onClose,
}: {
  open: boolean
  onClose: () => void
}) {
  return (
    <>
      {open && (
        <div
          className="fixed inset-0 z-30 bg-foreground/40 backdrop-blur-sm lg:hidden"
          onClick={onClose}
          aria-hidden
        />
      )}
      <aside
        className={cn(
          'fixed inset-y-0 left-0 z-40 flex w-64 flex-col border-r border-sidebar-border bg-sidebar transition-transform lg:static lg:translate-x-0',
          open ? 'translate-x-0' : '-translate-x-full',
        )}
      >
        <div className="flex h-16 items-center justify-between gap-2 border-b border-sidebar-border px-5">
          <div className="flex items-center gap-2.5">
            <div className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
              <Zap className="size-4.5" strokeWidth={2.5} />
            </div>
            <div className="flex flex-col leading-none">
              <span className="text-sm font-semibold tracking-tight">NovaGate</span>
              <span className="text-[11px] text-muted-foreground">AI 网关</span>
            </div>
          </div>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-muted-foreground hover:bg-muted lg:hidden"
            aria-label="关闭菜单"
          >
            <X className="size-5" />
          </button>
        </div>

        <nav className="flex-1 space-y-1 overflow-y-auto p-3">
          {nav.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              onClick={onClose}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors',
                  isActive
                    ? 'bg-sidebar-accent text-sidebar-accent-foreground'
                    : 'text-muted-foreground hover:bg-sidebar-accent/50 hover:text-sidebar-foreground',
                )
              }
            >
              <item.icon className="size-4.5 shrink-0" />
              {item.label}
            </NavLink>
          ))}
        </nav>

        <div className="border-t border-sidebar-border p-3">
          <div className="rounded-lg bg-accent/60 p-3">
            <div className="flex items-center gap-2 text-sm font-medium text-accent-foreground">
              <Zap className="size-4" />
              Pro 套餐
            </div>
            <p className="mt-1 text-xs text-muted-foreground">
              本月已用 60% 预算,余额充足。
            </p>
          </div>
        </div>
      </aside>
    </>
  )
}
