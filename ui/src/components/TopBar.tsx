import { useTranslation } from 'react-i18next';
import { useAuth } from '@/store/auth';
import { useLang } from '@/store/lang';
import { useTheme } from '@/store/theme';
import { useNavigate } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  Languages,
  Moon,
  Sun,
  Monitor,
  LogOut,
  User,
} from 'lucide-react';

export function TopBar() {
  const { t } = useTranslation();
  const { userName, clear } = useAuth();
  const { lang, setLang } = useLang();
  const { mode, resolved, setMode } = useTheme();
  const navigate = useNavigate();

  const handleLogout = () => {
    clear();
    navigate('/login');
  };

  const ThemeIcon = resolved === 'dark' ? Moon : Sun;

  return (
    <header className="h-14 border-b bg-background/70 glass flex items-center justify-between px-6 sticky top-0 z-20">
      <div />
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={() => setLang(lang === 'zh' ? 'en' : 'zh')}>
          <Languages className="h-4 w-4 mr-1" />
          {lang === 'zh' ? 'EN' : '中文'}
        </Button>
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex shrink-0 items-center justify-center rounded-md border border-input bg-transparent px-3 py-1.5 text-sm font-medium text-foreground shadow-sm hover:bg-accent hover:text-accent-foreground outline-none">
            <ThemeIcon className="h-4 w-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setMode('light')}>
              <Sun className="h-4 w-4 mr-2" />
              {t('theme.light')}{mode === 'light' && ' ✓'}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setMode('dark')}>
              <Moon className="h-4 w-4 mr-2" />
              {t('theme.dark')}{mode === 'dark' && ' ✓'}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setMode('system')}>
              <Monitor className="h-4 w-4 mr-2" />
              {t('theme.system')}{mode === 'system' && ' ✓'}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex shrink-0 items-center justify-center gap-2 rounded-md border border-input bg-transparent px-3 py-1.5 text-sm font-medium text-foreground shadow-sm hover:bg-accent hover:text-accent-foreground outline-none">
            <User className="h-4 w-4" />
            <span className="text-sm">{userName}</span>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={handleLogout}>
              <LogOut className="h-4 w-4 mr-2" />
              {t('nav.logout')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
