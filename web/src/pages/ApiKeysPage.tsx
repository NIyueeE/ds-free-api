import { useState } from 'react';
import useSWR from 'swr';
import { apiFetch, apiCreateKey, apiDeleteKey, type ApiKeyEntry, type StatsSnapshot, type KeyUsageSnapshot } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { Key, Plus, Trash2, Copy, Check } from 'lucide-react';

export function ApiKeysPage() {
  const { data: keys, mutate } = useSWR<ApiKeyEntry[]>(
    '/admin/api/keys',
    (url: string) => apiFetch<ApiKeyEntry[]>(url),
  );
  const { data: statsData } = useSWR<StatsSnapshot>(
    '/admin/api/stats',
    (url: string) => apiFetch<StatsSnapshot>(url),
  );
  const [showCreate, setShowCreate] = useState(false);
  const [description, setDescription] = useState('');
  const [newKey, setNewKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  const handleCreate = async () => {
    if (!description.trim()) return;
    try {
      const res = await apiCreateKey(description.trim());
      setNewKey(res.key);
      setDescription('');
      mutate();
    } catch {
      // error handled by SWR
    }
  };

  const handleDelete = async (key: string) => {
    if (!confirm('确定删除此 API Key？删除后不可恢复。')) return;
    setDeleting(key);
    try {
      await apiDeleteKey(key);
      mutate();
    } catch {
      // error handled
    }
    setDeleting(null);
  };

  const handleCopy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Fallback for non-HTTPS / insecure contexts
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.left = '-9999px';
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const closeNewKey = () => {
    setNewKey(null);
    setShowCreate(false);
  };

  // Match masked key from stats to key entry
  const getKeyUsage = (maskedKey: string): KeyUsageSnapshot | undefined => {
    if (!statsData?.keys) return undefined;
    // The masked key format is "first8chars***"
    return statsData.keys[maskedKey];
  };

  const formatTokens = (n: number): string => {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
    return String(n);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold flex items-center gap-2">
          <Key className="h-6 w-6" />
          API Keys
        </h1>
        <Button onClick={() => setShowCreate(true)} disabled={showCreate || newKey !== null}>
          <Plus className="h-4 w-4 mr-1" />
          新建
        </Button>
      </div>

      {/* New key display */}
      {newKey && (
        <Card className="border-green-200 bg-green-50/50">
          <CardHeader>
            <CardTitle className="text-base text-green-700">API Key 已创建</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <p className="text-sm text-muted-foreground">
              请立即复制，此 Key 仅显示一次：
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 bg-green-100 text-green-800 px-3 py-2 rounded text-sm font-mono break-all">
                {newKey}
              </code>
              <Button variant="outline" size="sm" onClick={() => handleCopy(newKey)}>
                {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
              </Button>
            </div>
            <Button variant="ghost" onClick={closeNewKey} className="mt-2">
              我已复制，关闭
            </Button>
          </CardContent>
        </Card>
      )}

      {/* Create form */}
      {showCreate && !newKey && (
        <Card>
          <CardContent className="pt-6 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="desc">描述</Label>
              <Input
                id="desc"
                placeholder="如：开发测试、生产环境"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>
            <div className="flex gap-2">
              <Button onClick={handleCreate} disabled={!description.trim()}>
                创建
              </Button>
              <Button variant="ghost" onClick={() => setShowCreate(false)}>
                取消
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Key list */}
      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Key（脱敏）</TableHead>
                <TableHead>描述</TableHead>
                <TableHead className="text-right">请求数</TableHead>
                <TableHead className="text-right">Token</TableHead>
                <TableHead className="w-40">创建时间</TableHead>
                <TableHead className="w-20 text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {keys?.map((entry) => (
                <TableRow key={entry.key}>
                  <TableCell>
                    <Badge variant="outline" className="font-mono text-xs">
                      {entry.key}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-sm">{entry.description}</TableCell>
                  <TableCell className="text-right text-sm">{getKeyUsage(entry.key)?.requests ?? '-'}</TableCell>
                  <TableCell className="text-right text-sm">
                    {(() => {
                      const u = getKeyUsage(entry.key);
                      if (!u) return '-';
                      const total = u.prompt_tokens + u.completion_tokens;
                      return formatTokens(total);
                    })()}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {new Date(entry.created_at * 1000).toLocaleString('zh-CN')}
                  </TableCell>
                  <TableCell className="text-right">
                    <Button
                      variant="ghost"
                      size="sm"
                      className="text-destructive hover:text-destructive"
                      onClick={() => handleDelete(entry.key)}
                      disabled={deleting === entry.key}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
              {!keys && (
                <TableRow>
                  <TableCell colSpan={6} className="text-center text-muted-foreground py-8">
                    加载中...
                  </TableCell>
                </TableRow>
              )}
              {keys && keys.length === 0 && (
                <TableRow>
                  <TableCell colSpan={6} className="text-center text-muted-foreground py-8">
                    暂无 API Key，点击上方「新建」创建
                  </TableCell>
                </TableRow>
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </div>
  );
}
