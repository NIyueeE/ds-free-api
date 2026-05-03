const TOKEN_KEY = 'ds-admin-token';

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Accept': 'application/json',
    'Content-Type': 'application/json',
    ...(init?.headers as Record<string, string> ?? {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    clearToken();
    throw new AuthError('Unauthorized');
  }
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new ApiError(res.status, body.error || `API error: ${res.status}`);
  }
  return res.json();
}

export class AuthError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'AuthError';
  }
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

// ── Auth API ──────────────────────────────────────────────────────────────

export interface LoginResponse {
  token: string;
}

export async function apiSetup(password: string): Promise<LoginResponse> {
  return apiFetch<LoginResponse>('/admin/api/setup', {
    method: 'POST',
    body: JSON.stringify({ password }),
  });
}

export async function apiLogin(password: string): Promise<LoginResponse> {
  return apiFetch<LoginResponse>('/admin/api/login', {
    method: 'POST',
    body: JSON.stringify({ password }),
  });
}

// ── API Key Management ────────────────────────────────────────────────────

export interface ApiKeyEntry {
  key: string;
  description: string;
  created_at: number;
}

export interface CreateKeyResponse {
  key: string;
}

export async function apiListKeys(): Promise<ApiKeyEntry[]> {
  return apiFetch<ApiKeyEntry[]>('/admin/api/keys');
}

export async function apiCreateKey(description: string): Promise<CreateKeyResponse> {
  return apiFetch<CreateKeyResponse>('/admin/api/keys', {
    method: 'POST',
    body: JSON.stringify({ description }),
  });
}

export async function apiDeleteKey(key: string): Promise<void> {
  await apiFetch(`/admin/api/keys/${encodeURIComponent(key)}`, {
    method: 'DELETE',
  });
}

// ── Account Management ────────────────────────────────────────────────────

export interface AddAccountRequest {
  email: string;
  mobile: string;
  area_code: string;
  password: string;
}

export async function apiAddAccount(req: AddAccountRequest): Promise<{ ok: boolean; id: string }> {
  return apiFetch<{ ok: boolean; id: string }>('/admin/api/accounts', {
    method: 'POST',
    body: JSON.stringify(req),
  });
}

export async function apiRemoveAccount(id: string): Promise<void> {
  await apiFetch(`/admin/api/accounts/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
}

export async function apiReloginAccount(id: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/admin/api/accounts/${encodeURIComponent(id)}/relogin`, {
    method: 'POST',
  });
}

export interface ReloadResult {
  ok: boolean;
  added: number;
  removed: number;
  failed: number;
}

export async function apiReloadConfig(): Promise<ReloadResult> {
  return apiFetch<ReloadResult>('/admin/api/reload', {
    method: 'POST',
  });
}

export interface RequestLog {
  timestamp: number;
  request_id: string;
  model: string;
  api_key: string;
  prompt_tokens: number;
  completion_tokens: number;
  latency_ms: number;
  success: boolean;
}

export interface RuntimeLogEntry {
  timestamp: string;
  level: string;
  target: string;
  message: string;
}

export interface RuntimeLogsResponse {
  total: number;
  offset: number;
  limit: number;
  logs: RuntimeLogEntry[];
}

export async function apiFetchRuntimeLogs(offset: number = 0, limit: number = 100): Promise<RuntimeLogsResponse> {
  return apiFetch<RuntimeLogsResponse>(`/admin/api/runtime-logs?offset=${offset}&limit=${limit}`);
}

export async function apiFetchLogs(limit?: number): Promise<RequestLog[]> {
  const path = limit ? `/admin/api/logs?limit=${limit}` : '/admin/api/logs';
  return apiFetch<RequestLog[]>(path);
}

// ── Data Types ────────────────────────────────────────────────────────────

export interface AccountStatus {
  email: string;
  mobile: string;
  state: string;
  last_released_ms: number;
  error_count: number;
}

export interface AdminStatusResponse {
  accounts: AccountStatus[];
  total: number;
  idle: number;
  busy: number;
  error: number;
  invalid: number;
}

export interface ModelStatsSnapshot {
  prompt_tokens: number;
  completion_tokens: number;
  requests: number;
}

export interface KeyUsageSnapshot {
  prompt_tokens: number;
  completion_tokens: number;
  requests: number;
}

export interface StatsSnapshot {
  total_requests: number;
  success_requests: number;
  failed_requests: number;
  avg_latency_ms: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  uptime_secs: number;
  models: Record<string, ModelStatsSnapshot>;
  keys: Record<string, KeyUsageSnapshot>;
}

export interface ModelInfo {
  id: string;
  object: string;
  created: number;
  owned_by: string;
}

export interface ModelListResponse {
  object: string;
  data: ModelInfo[];
}

export interface ServerConfigView {
  host: string;
  port: number;
}

export interface DeepSeekConfigView {
  api_base: string;
  model_types: string[];
  max_input_tokens: number[];
  max_output_tokens: number[];
}

export interface AccountView {
  email: string;
  mobile: string;
  area_code: string;
  password: string;
}

export interface AdminConfigResponse {
  server: ServerConfigView;
  deepseek: DeepSeekConfigView;
  accounts: AccountView[];
}
