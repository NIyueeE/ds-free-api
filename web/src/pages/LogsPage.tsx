import { useState } from 'react';
import useSWR from 'swr';
import {
  apiFetch,
  apiFetchRuntimeLogs,
  type RequestLog,
  type RuntimeLogsResponse,
} from '@/lib/api';
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
import { ScrollText, Terminal, ChevronLeft, ChevronRight } from 'lucide-react';

const PAGE_SIZE = 50;

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString('zh-CN');
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatLatency(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${ms}ms`;
}

const levelVariant = (level: string) => {
  switch (level) {
    case 'ERROR': return 'destructive';
    case 'WARN': return 'secondary';
    case 'INFO': return 'outline';
    case 'DEBUG': return 'outline';
    case 'TRACE': return 'outline';
    default: return 'outline';
  }
};

const levelColor = (level: string) => {
  switch (level) {
    case 'ERROR': return 'bg-red-500/15 text-red-700 border-red-200';
    case 'WARN': return 'bg-yellow-500/15 text-yellow-700 border-yellow-200';
    case 'INFO': return 'bg-blue-500/15 text-blue-700 border-blue-200';
    case 'DEBUG': return 'bg-gray-500/15 text-gray-600 border-gray-200';
    case 'TRACE': return 'bg-gray-500/10 text-gray-500 border-gray-200';
    default: return '';
  }
};

// ── 请求日志 Tab ──────────────────────────────────────────────────────────

function RequestLogsTab() {
  const { data: logs } = useSWR<RequestLog[]>(
    '/admin/api/logs?limit=100',
    (url: string) => apiFetch<RequestLog[]>(url),
    { refreshInterval: 5000 },
  );

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm text-muted-foreground">
          最近 100 条请求（每 5 秒自动刷新）
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>时间</TableHead>
              <TableHead>模型</TableHead>
              <TableHead>API Key</TableHead>
              <TableHead className="text-right">输入</TableHead>
              <TableHead className="text-right">输出</TableHead>
              <TableHead className="text-right">延迟</TableHead>
              <TableHead>状态</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {logs?.map((log, i) => (
              <TableRow key={i}>
                <TableCell className="text-xs text-muted-foreground whitespace-nowrap">
                  {formatTime(log.timestamp)}
                </TableCell>
                <TableCell className="font-mono text-xs">{log.model}</TableCell>
                <TableCell className="font-mono text-xs text-muted-foreground">{log.api_key}</TableCell>
                <TableCell className="text-right text-xs text-blue-600">{formatTokens(log.prompt_tokens)}</TableCell>
                <TableCell className="text-right text-xs text-emerald-600">{formatTokens(log.completion_tokens)}</TableCell>
                <TableCell className="text-right text-xs">{formatLatency(log.latency_ms)}</TableCell>
                <TableCell>
                  {log.success ? (
                    <Badge variant="outline" className="bg-green-500/15 text-green-700 border-green-200 text-xs">
                      成功
                    </Badge>
                  ) : (
                    <Badge variant="outline" className="bg-red-500/15 text-red-700 border-red-200 text-xs">
                      失败
                    </Badge>
                  )}
                </TableCell>
              </TableRow>
            ))}
            {!logs && (
              <TableRow>
                <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                  加载中...
                </TableCell>
              </TableRow>
            )}
            {logs && logs.length === 0 && (
              <TableRow>
                <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                  暂无请求日志
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

// ── 运行日志 Tab ──────────────────────────────────────────────────────────

function RuntimeLogsTab() {
  const [page, setPage] = useState(0);
  const offset = page * PAGE_SIZE;

  const { data, isLoading } = useSWR<RuntimeLogsResponse>(
    `/admin/api/runtime-logs?offset=${offset}&limit=${PAGE_SIZE}`,
    () => apiFetchRuntimeLogs(offset, PAGE_SIZE),
    { refreshInterval: 3000 },
  );

  const totalPages = data ? Math.ceil(data.total / PAGE_SIZE) : 0;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm text-muted-foreground">
          运行日志（共 {data?.total ?? '-'} 条，每 3 秒自动刷新）
        </CardTitle>
      </CardHeader>
      <CardContent className="p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-44">时间</TableHead>
              <TableHead className="w-20">级别</TableHead>
              <TableHead className="w-40">模块</TableHead>
              <TableHead>消息</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {data?.logs.map((log, i) => (
              <TableRow key={i}>
                <TableCell className="text-xs text-muted-foreground whitespace-nowrap font-mono">
                  {log.timestamp}
                </TableCell>
                <TableCell>
                  <Badge variant={levelVariant(log.level)} className={`text-xs ${levelColor(log.level)}`}>
                    {log.level}
                  </Badge>
                </TableCell>
                <TableCell className="text-xs font-mono text-muted-foreground">
                  {log.target}
                </TableCell>
                <TableCell className="text-xs break-all">
                  {log.message}
                </TableCell>
              </TableRow>
            ))}
            {isLoading && (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                  加载中...
                </TableCell>
              </TableRow>
            )}
            {data && data.logs.length === 0 && (
              <TableRow>
                <TableCell colSpan={4} className="text-center text-muted-foreground py-8">
                  暂无运行日志
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>

        {/* Pagination */}
        {totalPages > 1 && (
          <div className="flex items-center justify-between px-4 py-3 border-t">
            <span className="text-sm text-muted-foreground">
              第 {page + 1} / {totalPages} 页
            </span>
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setPage(p => Math.max(0, p - 1))}
                disabled={page === 0}
              >
                <ChevronLeft className="h-4 w-4 mr-1" />
                上一页
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setPage(p => Math.min(totalPages - 1, p + 1))}
                disabled={page >= totalPages - 1}
              >
                下一页
                <ChevronRight className="h-4 w-4 ml-1" />
              </Button>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// ── 主页面 ────────────────────────────────────────────────────────────────

export function LogsPage() {
  const [tab, setTab] = useState<'request' | 'runtime'>('request');

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold flex items-center gap-2">
        <ScrollText className="h-6 w-6" />
        日志
      </h1>

      {/* Tab switcher */}
      <div className="flex gap-2">
        <Button
          variant={tab === 'request' ? 'default' : 'outline'}
          size="sm"
          onClick={() => setTab('request')}
        >
          <ScrollText className="h-4 w-4 mr-1" />
          请求日志
        </Button>
        <Button
          variant={tab === 'runtime' ? 'default' : 'outline'}
          size="sm"
          onClick={() => setTab('runtime')}
        >
          <Terminal className="h-4 w-4 mr-1" />
          运行日志
        </Button>
      </div>

      {tab === 'request' ? <RequestLogsTab /> : <RuntimeLogsTab />}
    </div>
  );
}
