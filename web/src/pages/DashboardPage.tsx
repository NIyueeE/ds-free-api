import useSWR, { mutate } from 'swr';
import { apiFetch, apiReloadConfig, type AdminStatusResponse, type StatsSnapshot } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
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
  RefreshCw,
} from 'lucide-react';

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}天 ${h}时 ${m}分`;
  if (h > 0) return `${h}时 ${m}分`;
  return `${m}分`;
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
      <h1 className="text-2xl font-bold">概览</h1>

      {/* Stats cards */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">总请求</CardTitle>
            <Activity className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{stats?.total_requests ?? '-'}</div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">成功率</CardTitle>
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
            <CardTitle className="text-sm font-medium text-muted-foreground">平均延迟</CardTitle>
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
            <CardTitle className="text-sm font-medium text-muted-foreground">运行时长</CardTitle>
            <Zap className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">
              {stats ? formatUptime(stats.uptime_secs) : '-'}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Token stats cards */}
      <div className="grid gap-4 md:grid-cols-3">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">总 Token</CardTitle>
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
            <CardTitle className="text-sm font-medium text-muted-foreground">输入 Token</CardTitle>
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
            <CardTitle className="text-sm font-medium text-muted-foreground">输出 Token</CardTitle>
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
              模型统计
            </CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>模型</TableHead>
                  <TableHead className="text-right">请求数</TableHead>
                  <TableHead className="text-right">输入 Token</TableHead>
                  <TableHead className="text-right">输出 Token</TableHead>
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
            账号池
            <Button
              variant="outline"
              size="sm"
              className="ml-auto"
              onClick={async () => {
                try {
                  const res = await apiReloadConfig();
                  alert(`重载完成：新增 ${res.added}，移除 ${res.removed}，失败 ${res.failed}`);
                  mutate('/admin/api/status');
                } catch {
                  alert('重载失败');
                }
              }}
            >
              <RefreshCw className="h-4 w-4 mr-1" />
              重载配置
            </Button>
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex gap-6">
            <div className="text-center">
              <div className="text-3xl font-bold">{status?.total ?? '-'}</div>
              <div className="text-sm text-muted-foreground">总数</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-green-600">{status?.idle ?? '-'}</div>
              <div className="text-sm text-muted-foreground">空闲</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-amber-500">{status?.busy ?? '-'}</div>
              <div className="text-sm text-muted-foreground">忙碌</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-yellow-500">{status?.error ?? '-'}</div>
              <div className="text-sm text-muted-foreground">异常</div>
            </div>
            <div className="text-center">
              <div className="text-3xl font-bold text-red-600">{status?.invalid ?? '-'}</div>
              <div className="text-sm text-muted-foreground">失效</div>
            </div>
          </div>
          <div className="mt-4 flex flex-wrap gap-2">
            {status?.accounts.map((a) => {
              const isBusy = a.state === 'busy';
              const isError = a.state === 'error';
              const isInvalid = a.state === 'invalid';
              const variant = isBusy ? 'default' : isError ? 'secondary' : isInvalid ? 'destructive' : 'secondary';
              const className = isBusy
                ? 'bg-amber-500/15 text-amber-700 border-amber-200'
                : isError
                ? 'bg-yellow-500/15 text-yellow-700 border-yellow-200'
                : isInvalid
                ? 'bg-red-500/15 text-red-700 border-red-200'
                : 'bg-green-500/15 text-green-700 border-green-200';
              return (
                <Badge key={a.email || a.mobile} variant={variant} className={className}>
                  {a.email || a.mobile}
                </Badge>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
