import useSWR from 'swr';
import { apiFetch, type AdminStatusResponse, type StatsSnapshot } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Badge } from '@/components/ui/badge';
import {
  Activity,
  Clock,
  CheckCircle,
  XCircle,
  Users,
  Zap,
  TrendingUp,
  Coins,
  Box,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

function formatUptime(secs: number, t: (key: string) => string): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}${t('dashboard.stats.days')} ${h}${t('dashboard.stats.hours')} ${m}${t('dashboard.stats.minutes')}`;
  if (h > 0) return `${h}${t('dashboard.stats.hours')} ${m}${t('dashboard.stats.minutes')}`;
  return `${m}${t('dashboard.stats.minutes')}`;
}

function formatLatency(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

export function DashboardPage() {
  const { t } = useTranslation();
  const { data: status } = useSWR<AdminStatusResponse>(
    '/admin/api/status',
    (url) => apiFetch<AdminStatusResponse>(url),
    { refreshInterval: 5000 }
  );
  const { data: stats } = useSWR<StatsSnapshot>(
    '/admin/api/stats',
    (url) => apiFetch<StatsSnapshot>(url),
    { refreshInterval: 5000 }
  );

  const successRate = stats
    ? stats.total_requests > 0
      ? ((stats.success_requests / stats.total_requests) * 100).toFixed(1)
      : '0.0'
    : '-';

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">{t('dashboard.title')}</h1>

      {/* Stats cards */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.totalRequests')}</CardTitle>
            <Activity className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{stats?.total_requests ?? '-'}</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.successRate')}</CardTitle>
            <TrendingUp className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{successRate}%</div>
            <div className="flex gap-2 mt-1">
              <span className="text-xs text-green-600 flex items-center gap-1">
                <CheckCircle className="h-3 w-3" />
                {stats?.success_requests ?? 0}
              </span>
              <span className="text-xs text-red-500 flex items-center gap-1">
                <XCircle className="h-3 w-3" />
                {stats?.failed_requests ?? 0}
              </span>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.avgLatency')}</CardTitle>
            <Clock className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats ? formatLatency(stats.avg_latency_ms) : '-'}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.uptime')}</CardTitle>
            <Zap className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats ? formatUptime(stats.uptime_secs, t) : '-'}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Token stats cards */}
      <div className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.totalTokens')}</CardTitle>
            <Coins className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats ? formatTokens(stats.total_prompt_tokens + stats.total_completion_tokens) : '-'}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.promptTokens')}</CardTitle>
            <Coins className="h-4 w-4 text-blue-400" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-blue-600">
              {stats ? formatTokens(stats.total_prompt_tokens) : '-'}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">{t('dashboard.stats.completionTokens')}</CardTitle>
            <Coins className="h-4 w-4 text-emerald-400" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-emerald-600">
              {stats ? formatTokens(stats.total_completion_tokens) : '-'}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Model stats table */}
      {stats?.models && Object.keys(stats.models).length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Box className="h-5 w-5" />
              {t('dashboard.stats.models')}
            </CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('dashboard.stats.model')}</TableHead>
                  <TableHead className="text-right">{t('dashboard.stats.requests')}</TableHead>
                  <TableHead className="text-right">{t('dashboard.stats.prompt')}</TableHead>
                  <TableHead className="text-right">{t('dashboard.stats.completion')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {Object.entries(stats.models).map(([model, ms]) => (
                  <TableRow key={model}>
                    <TableCell className="font-mono text-sm">{model}</TableCell>
                    <TableCell className="text-right">{ms.requests}</TableCell>
                    <TableCell className="text-right text-blue-600">{formatTokens(ms.prompt_tokens)}</TableCell>
                    <TableCell className="text-right text-emerald-600">{formatTokens(ms.completion_tokens)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {/* Account pool summary */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Users className="h-5 w-5" />
            {t('dashboard.accountPool.title')}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex gap-6">
            <div className="text-center">
              <div className="text-3xl font-bold">{status?.total ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.total')}</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-green-600">{status?.idle ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.idle')}</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-amber-500">{status?.busy ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.busy')}</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-yellow-500">{status?.error ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.error')}</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-red-600">{status?.invalid ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.invalid')}</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-purple-600">{status?.muted ?? '-'}</div>
              <div className="text-sm text-muted-foreground">{t('dashboard.accountPool.muted')}</div>
            </div>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            {status?.accounts.map((a) => {
              const isBusy = a.state === 'busy';
              const isError = a.state === 'error';
              const isInvalid = a.state === 'invalid';
              const isMuted = a.state === 'muted';
              let variant = 'secondary';
              let className = 'bg-green-500/15 text-green-700 border-green-200';
              let displayText = a.email || a.mobile;

              if (isBusy) {
                variant = 'default';
                className = 'bg-amber-500/15 text-amber-700 border-amber-200';
              } else if (isError) {
                variant = 'secondary';
                className = 'bg-yellow-500/15 text-yellow-700 border-yellow-200';
              } else if (isInvalid) {
                variant = 'destructive';
                className = 'bg-red-500/15 text-red-700 border-red-200';
              } else if (isMuted) {
                variant = 'secondary';
                className = 'bg-purple-500/15 text-purple-700 border-purple-200';
                // 显示解封倒计时
                if (a.mute_until && a.mute_until > 0) {
                  const now = Date.now() / 1000;
                  const remainingSecs = Math.max(0, a.mute_until - now);
                  if (remainingSecs > 0) {
                    const hours = Math.floor(remainingSecs / 3600);
                    const minutes = Math.floor((remainingSecs % 3600) / 60);
                    const seconds = Math.floor(remainingSecs % 60);
                    let remainingStr = '';
                    if (hours > 0) remainingStr = `${hours}h ${minutes}m`;
                    else if (minutes > 0) remainingStr = `${minutes}m ${seconds}s`;
                    else remainingStr = `${seconds}s`;
                    displayText = `${a.email || a.mobile} (${remainingStr})`;
                  }
                }
              }

              return (
                <Badge key={a.email || a.mobile} variant={variant} className={className}>
                  {displayText}
                </Badge>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
