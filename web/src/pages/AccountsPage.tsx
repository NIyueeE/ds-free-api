import { useState } from 'react';
import useSWR from 'swr';
import {
  apiFetch,
  apiAddAccount,
  apiRemoveAccount,
  apiReloginAccount,
  type AdminStatusResponse,
  type AddAccountRequest,
} from '@/lib/api';
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
import { Users, Plus, Trash2, Loader2, RotateCw } from 'lucide-react';

export function AccountsPage() {
  const { data, mutate } = useSWR<AdminStatusResponse>(
    '/admin/api/status',
    (url: string) => apiFetch<AdminStatusResponse>(url),
  );
  const [showAdd, setShowAdd] = useState(false);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const [relogging, setRelogging] = useState<string | null>(null);
  const [form, setForm] = useState<AddAccountRequest>({
    email: '',
    mobile: '',
    area_code: '+86',
    password: '',
  });
  const [error, setError] = useState('');

  const handleAdd = async () => {
    if (form.email.trim() === '' && form.mobile.trim() === '') {
      setError('email 和 mobile 至少填一项');
      return;
    }
    if (form.password.trim() === '') {
      setError('密码不能为空');
      return;
    }
    setAdding(true);
    setError('');
    try {
      await apiAddAccount({
        email: form.email.trim(),
        mobile: form.mobile.trim(),
        area_code: form.area_code.trim(),
        password: form.password.trim(),
      });
      setForm({ email: '', mobile: '', area_code: '+86', password: '' });
      setShowAdd(false);
      mutate();
    } catch (e) {
      setError(e instanceof Error ? e.message : '添加失败');
    }
    setAdding(false);
  };

  const handleRelogin = async (id: string) => {
    setRelogging(id);
    try {
      const res = await apiReloginAccount(id);
      if (res.ok) {
        alert(`账号 ${id} 重新登录成功`);
        mutate();
      }
    } catch (e) {
      alert(e instanceof Error ? e.message : '重登失败');
    }
    setRelogging(null);
  };

  const handleRemove = async (id: string) => {
    if (!confirm(`确定移除账号 ${id}？`)) return;
    setRemoving(id);
    try {
      await apiRemoveAccount(id);
      mutate();
    } catch (e) {
      alert(e instanceof Error ? e.message : '移除失败');
    }
    setRemoving(null);
  };

  const accounts = data?.accounts ?? [];

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold flex items-center gap-2">
          <Users className="h-6 w-6" />
          账号池
        </h1>
        <Button onClick={() => setShowAdd(true)} disabled={showAdd}>
          <Plus className="h-4 w-4 mr-1" />
          添加账号
        </Button>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-5 gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-muted-foreground">总计</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{data?.total ?? '-'}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-muted-foreground">空闲</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-green-600">{data?.idle ?? '-'}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-muted-foreground">忙碌</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-orange-500">{data?.busy ?? '-'}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-muted-foreground">异常</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-yellow-500">{data?.error ?? '-'}</div>
          </CardContent>
        </Card>
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-sm text-muted-foreground">失效</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold text-red-600">{data?.invalid ?? '-'}</div>
          </CardContent>
        </Card>
      </div>

      {/* Add form */}
      {showAdd && (
        <Card>
          <CardHeader>
            <CardTitle className="text-lg">添加账号</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="email">邮箱</Label>
                <Input
                  id="email"
                  placeholder="user@example.com"
                  value={form.email}
                  onChange={(e) => setForm({ ...form, email: e.target.value })}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="mobile">手机号</Label>
                <div className="flex gap-2">
                  <Input
                    id="area_code"
                    className="w-20"
                    placeholder="+86"
                    value={form.area_code}
                    onChange={(e) => setForm({ ...form, area_code: e.target.value })}
                  />
                  <Input
                    className="flex-1"
                    placeholder="13800138000"
                    value={form.mobile}
                    onChange={(e) => setForm({ ...form, mobile: e.target.value })}
                  />
                </div>
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="password">密码</Label>
              <Input
                id="password"
                type="password"
                placeholder="DeepSeek 账号密码"
                value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
              />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <div className="flex gap-2">
              <Button onClick={handleAdd} disabled={adding}>
                {adding && <Loader2 className="h-4 w-4 mr-1 animate-spin" />}
                添加并初始化
              </Button>
              <Button variant="ghost" onClick={() => { setShowAdd(false); setError(''); }}>
                取消
              </Button>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Account list */}
      <Card>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>标识</TableHead>
                <TableHead>邮箱</TableHead>
                <TableHead>手机号</TableHead>
                <TableHead className="w-24">状态</TableHead>
                <TableHead className="w-16 text-right">重试</TableHead>
                <TableHead className="w-20 text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {accounts.map((a) => {
                    const id = a.email || a.mobile;
                    const stateLabel = a.state === 'idle' ? '空闲' : a.state === 'busy' ? '忙碌' : a.state === 'error' ? '异常' : '失效';
                    const badgeVariant = a.state === 'idle' ? 'default' : a.state === 'busy' ? 'outline' : a.state === 'error' ? 'secondary' : 'destructive';
                    return (
                  <TableRow key={id}>
                    <TableCell className="font-medium">{id}</TableCell>
                    <TableCell className="text-sm">{a.email || '-'}</TableCell>
                    <TableCell className="text-sm">{a.mobile || '-'}</TableCell>
                    <TableCell>
                      <Badge variant={badgeVariant}>
                        {stateLabel}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-right text-sm text-muted-foreground">
                      {a.state === 'error' || a.state === 'invalid' ? a.error_count : '-'}
                    </TableCell>
                    <TableCell className="text-right">
                      {(a.state === 'error' || a.state === 'invalid') && (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-yellow-600 hover:text-yellow-700 mr-1"
                          onClick={() => handleRelogin(id)}
                          disabled={relogging === id}
                          title="重新登录"
                        >
                          {relogging === id ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            <RotateCw className="h-4 w-4" />
                          )}
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:text-destructive"
                        onClick={() => handleRemove(id)}
                        disabled={removing === id || a.state === 'busy'}
                        title={a.state === 'busy' ? '账号忙碌中，无法移除' : '移除账号'}
                      >
                        {removing === id ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <Trash2 className="h-4 w-4" />
                        )}
                      </Button>
                    </TableCell>
                  </TableRow>
                );
              })}
              {!data && (
                <TableRow>
                  <TableCell colSpan={6} className="text-center text-muted-foreground py-8">
                    加载中...
                  </TableCell>
                </TableRow>
              )}
              {data && accounts.length === 0 && (
                <TableRow>
                  <TableCell colSpan={6} className="text-center text-muted-foreground py-8">
                    暂无账号
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
