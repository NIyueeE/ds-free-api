import useSWR from 'swr';
import { apiFetch, type ModelListResponse } from '@/lib/api';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Boxes } from 'lucide-react';

export function ModelsPage() {
  const { data: models } = useSWR<ModelListResponse>(
    '/admin/api/models',
    (url: string) => apiFetch<ModelListResponse>(url),
  );

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold flex items-center gap-2">
        <Boxes className="h-6 w-6" />
        模型列表
      </h1>

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        {models?.data.map((model) => (
          <Card key={model.id}>
            <CardHeader className="pb-2">
              <CardTitle className="text-base flex items-center justify-between">
                <span>{model.id}</span>
                <Badge variant="secondary" className="text-xs">
                  {model.owned_by}
                </Badge>
              </CardTitle>
            </CardHeader>
            <CardContent>
              <div className="text-sm text-muted-foreground space-y-1">
                <div>类型: {model.object}</div>
              </div>
            </CardContent>
          </Card>
        ))}
        {!models && (
          <div className="col-span-full text-center text-muted-foreground py-8">
            加载中...
          </div>
        )}
        {models && models.data.length === 0 && (
          <div className="col-span-full text-center text-muted-foreground py-8">
            暂无模型
          </div>
        )}
      </div>
    </div>
  );
}
