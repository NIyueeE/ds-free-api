import { NavLink, Outlet, useNavigate } from 'react-router-dom';
import { useAuth } from '@/lib/use-auth';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { LanguageSwitcher } from '@/components/LanguageSwitcher';
import { ThemeSwitcher } from '@/components/ThemeSwitcher';
import {
  LayoutDashboard,
  Boxes,
  Settings,
  LogOut,
  ScrollText,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

export function Layout() {
  const { t } = useTranslation();
  const { logout } = useAuth();
  const navigate = useNavigate();

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

  const navItems = [
    { to: '/', icon: LayoutDashboard, label: t('nav.dashboard') },
    { to: '/models', icon: Boxes, label: t('nav.models') },
    { to: '/config', icon: Settings, label: t('nav.config') },
    { to: '/logs', icon: ScrollText, label: t('nav.logs') },
  ];

  return (
    <div className="min-h-screen flex bg-background">
      {/* Sidebar */}
      <aside className="w-56 border-r bg-card flex flex-col">
        <div className="p-4 flex items-center gap-2">
          <img src="/admin/favicon.svg" alt="Logo" className="h-6 w-6" />
          <span className="font-bold text-lg">DS Free API</span>
        </div>
        <Separator />
        <nav className="flex-1 p-2 space-y-1">
          {navItems.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-primary/10 text-primary'
                    : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
                }`
              }
            >
              <Icon className="h-4 w-4" />
              {label}
            </NavLink>
          ))}
        </nav>
        <Separator />
        <div className="p-2 space-y-2">
          <LanguageSwitcher />
          <ThemeSwitcher />
          <Button
            variant="ghost"
            className="w-full justify-start gap-3 text-muted-foreground"
            onClick={handleLogout}
          >
            <LogOut className="h-4 w-4" />
            {t('nav.logout')}
          </Button>
        </div>
      </aside>

      {/* Main content */}
      <main className="flex-1 overflow-auto">
        <div className="p-6 w-full">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
