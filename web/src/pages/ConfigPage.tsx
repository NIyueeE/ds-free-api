import useSWR from 'swr';
import { apiFetch, type AdminConfigResponse } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Separator } from '@/components/ui/separator';
import { Settings, Server, Cpu, User } from 'lucide-react';

export function ConfigPage() {
  const { data: config } = useSWR<AdminConfigResponse>(
    '/admin/api/config',
    (url: string) => apiFetch<AdminConfigResponse>(url),
  );

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold flex items-center gap-2">
        <Settings className="h-6 w-6" />
        配置查看
      </h1>

      {/* Server config */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Server className="h-5 w-5" />
            服务器
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid grid-cols-2 gap-2 text-sm">
            <span className="text-muted-foreground">监听地址</span>
            <span className="font-mono">{config?.server.host}:{config?.server.port}</span>
          </div>
        </CardContent>
      </Card>

      {/* DeepSeek config */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <Cpu className="h-5 w-5" />
            DeepSeek
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid grid-cols-2 gap-2 text-sm">
            <span className="text-muted-foreground">API Base</span>
            <span className="font-mono text-xs break-all">{config?.deepseek.api_base}</span>
          </div>
          <Separator />
          <div className="text-sm">
            <span className="text-muted-foreground">模型类型</span>
            <div className="mt-2 flex flex-wrap gap-2">
              {config?.deepseek.model_types.map((t, i) => (
                <Badge key={i} variant="secondary">
                  {t}
                  <span className="ml-1 text-xs text-muted-foreground">
                    (in: {config.deepseek.max_input_tokens[i]?.toLocaleString()}, out: {config.deepseek.max_output_tokens[i]?.toLocaleString()})
                  </span>
                </Badge>
              ))}
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Accounts config */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-lg">
            <User className="h-5 w-5" />
            账号配置
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-2">
            {config?.accounts.map((a, i) => (
              <div key={i} className="flex items-center gap-3 text-sm py-1">
                <Badge variant="outline" className="font-mono">
                  {a.email || a.mobile}
                </Badge>
                {a.area_code && (
                  <span className="text-muted-foreground">区号: {a.area_code}</span>
                )}
                <span className="text-muted-foreground">密码: ••••••</span>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
