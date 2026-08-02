import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import { useLogout } from '@fluxeme/shared/src/api/auth';
import { Button } from '@fluxeme/shared/src/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@fluxeme/shared/src/components/ui/dropdown-menu';
import { usePermission } from '@fluxeme/shared/src/permissions';
import { useAuth } from '@fluxeme/shared/src/store/auth';
import { useLang } from '@fluxeme/shared/src/store/lang';
import { useTheme } from '@fluxeme/shared/src/store/theme';
import { Languages, LogOut, Monitor, Moon, Sun, User } from 'lucide-react';

type TopBarProps = {
  adminEntryPath?: string;
  userEntryPath?: string;
};

export function TopBar({ adminEntryPath, userEntryPath }: TopBarProps) {
  const { t } = useTranslation();
  const { userName, clear } = useAuth();
  const { lang, setLang } = useLang();
  const { mode, resolved, setMode } = useTheme();
  const isAdmin = usePermission('admin:dashboard');
  const logout = useLogout();
  const navigate = useNavigate();

  const handleLogout = () => {
    logout.mutate(undefined, {
      onSuccess: () => {
        clear();
        navigate('/login');
      },
      onError: (error) => {
        if (error.message === 'unauthorized') {
          clear();
          navigate('/login');
          return;
        }

        toast.error(error.message);
      },
    });
  };

  const ThemeIcon = resolved === 'dark' ? Moon : Sun;
  const shouldShowAdminEntry = isAdmin && adminEntryPath;
  const shouldShowUserEntry = isAdmin && userEntryPath;

  return (
    <header className="glass sticky top-0 z-20 flex h-14 items-center justify-between border-b bg-background/70 px-6">
      <div />
      <div className="flex items-center gap-2">
        {shouldShowUserEntry ? (
          <Button variant="outline" size="sm" onClick={() => navigate(userEntryPath)}>
            {t('nav.userEntry')}
          </Button>
        ) : null}
        {shouldShowAdminEntry ? (
          <Button variant="outline" size="sm" onClick={() => navigate(adminEntryPath)}>
            {t('nav.adminEntry')}
          </Button>
        ) : null}
        <Button variant="ghost" size="sm" onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}>
          <Languages className="mr-1 h-4 w-4" />
          {lang === 'zh' ? 'EN' : '中文'}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger
            aria-label={t('theme.change')}
            className="inline-flex shrink-0 items-center justify-center rounded-md border border-input bg-transparent px-3 py-1.5 text-sm font-medium text-foreground shadow-sm outline-none hover:bg-accent hover:text-accent-foreground"
          >
            <ThemeIcon className="h-4 w-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setMode('light')}>
              <Sun className="mr-2 h-4 w-4" />
              {t('theme.light')}
              {mode === 'light' && ' ✓'}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setMode('dark')}>
              <Moon className="mr-2 h-4 w-4" />
              {t('theme.dark')}
              {mode === 'dark' && ' ✓'}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setMode('system')}>
              <Monitor className="mr-2 h-4 w-4" />
              {t('theme.system')}
              {mode === 'system' && ' ✓'}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex shrink-0 items-center justify-center gap-2 rounded-md border border-input bg-transparent px-3 py-1.5 text-sm font-medium text-foreground shadow-sm outline-none hover:bg-accent hover:text-accent-foreground">
            <User className="h-4 w-4" />
            <span className="text-sm">{userName}</span>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={handleLogout}>
              <LogOut className="mr-2 h-4 w-4" />
              {t('nav.logout')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
