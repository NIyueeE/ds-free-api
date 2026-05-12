import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Monitor, Moon, Sun } from 'lucide-react';

type ThemePreference = 'system' | 'light' | 'dark';

const STORAGE_KEY = 'theme-preference';

const themeConfig = {
  system: { icon: Monitor, labelKey: 'theme.system' },
  light: { icon: Sun, labelKey: 'theme.light' },
  dark: { icon: Moon, labelKey: 'theme.dark' },
} as const;

function getStoredTheme(): ThemePreference {
  if (typeof window === 'undefined') {
    return 'system';
  }

  const value = localStorage.getItem(STORAGE_KEY);
  if (value === 'light' || value === 'dark' || value === 'system') {
    return value;
  }
  return 'system';
}

function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function applyTheme(theme: ThemePreference) {
  const effectiveTheme = theme === 'system' ? getSystemTheme() : theme;
  document.documentElement.classList.toggle('dark', effectiveTheme === 'dark');
}

export function ThemeSwitcher() {
  const { t } = useTranslation();
  const [theme, setTheme] = useState<ThemePreference>(() => getStoredTheme());

  useEffect(() => {
    applyTheme(theme);
    localStorage.setItem(STORAGE_KEY, theme);
  }, [theme]);

  useEffect(() => {
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const listener = () => {
      if (theme === 'system') {
        applyTheme('system');
      }
    };
    mediaQuery.addEventListener('change', listener);
    return () => mediaQuery.removeEventListener('change', listener);
  }, [theme]);

  const nextTheme: ThemePreference = theme === 'system' ? 'dark' : theme === 'dark' ? 'light' : 'system';
  const { icon: Icon, labelKey } = themeConfig[theme];

  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={() => setTheme(nextTheme)}
      className="w-full justify-start gap-3 text-muted-foreground"
      title={t('theme.toggle')}
    >
      <Icon className="h-4 w-4" />
      {t(labelKey)}
    </Button>
  );
}
