import { NavLink, Outlet, useNavigate } from 'react-router-dom';
import { useAuth } from '@/lib/auth';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import {
  LayoutDashboard,
  Users,
  Boxes,
  Settings,
  LogOut,
  Key,
  ScrollText,
} from 'lucide-react';

const navItems = [
  { to: '/', icon: LayoutDashboard, label: '概览' },
  { to: '/accounts', icon: Users, label: '账号池' },
  { to: '/keys', icon: Key, label: 'API Keys' },
  { to: '/models', icon: Boxes, label: '模型' },
  { to: '/config', icon: Settings, label: '配置' },
  { to: '/logs', icon: ScrollText, label: '日志' },
];

export function Layout() {
  const { logout } = useAuth();
  const navigate = useNavigate();

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

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
        <div className="p-2">
          <Button
            variant="ghost"
            className="w-full justify-start gap-3 text-muted-foreground"
            onClick={handleLogout}
          >
            <LogOut className="h-4 w-4" />
            退出
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
