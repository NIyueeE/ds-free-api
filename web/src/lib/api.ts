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

let onUnauthorized: (() => void) | null = null;

/** 注册 401 回调：收到 401 时自动调用（用于 AuthProvider 同步 token 状态） */
export function setOnUnauthorized(cb: (() => void) | null) {
  onUnauthorized = cb;
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
    onUnauthorized?.();
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

// ── Data Types ────────────────────────────────────────────────────────────

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

export interface AccountStatus {
  email: string;
  mobile: string;
  state: string;
  last_released_ms: number;
  error_count: number;
  mute_until: number;
}

export interface AdminStatusResponse {
  accounts: AccountStatus[];
  total: number;
  idle: number;
  busy: number;
  error: number;
  invalid: number;
  muted: number;
}

export interface StatsSnapshot {
  total_requests: number;
  success_requests: number;
  failed_requests: number;
  avg_latency_ms: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  uptime_secs: number;
  models: Record<string, { prompt_tokens: number; completion_tokens: number; requests: number }>;
  keys: Record<string, { prompt_tokens: number; completion_tokens: number; requests: number }>;
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

// ── Config Types (mirrors backend response) ───────────────────────────────

export interface ServerConfig {
  host: string;
  port: number;
  cors_origins: string[];
}

export interface ToolCallTagConfig {
  extra_starts: string[];
  extra_ends: string[];
}

export interface DeepSeekConfig {
  api_base: string;
  wasm_url: string;
  user_agent: string;
  client_version: string;
  client_platform: string;
  client_locale: string;
  model_types: string[];
  max_input_tokens: number[];
  max_output_tokens: number[];
  input_character_limits: number[];
  model_aliases: string[];
  tool_call: ToolCallTagConfig;
}

export interface ProxyConfig {
  url: string | null;
}

export interface AdminConfigResponse {
  password_set: boolean;
  jwt_issued_at: number;
}

export interface AccountEntry {
  email: string;
  mobile: string;
  area_code: string;
  password: string;
}

export interface ApiKeyEntry {
  key: string;
  description: string;
}

export interface FullConfig {
  server: ServerConfig;
  deepseek: DeepSeekConfig;
  proxy: ProxyConfig;
  admin: AdminConfigResponse;
  accounts: AccountEntry[];
  api_keys: ApiKeyEntry[];
}

// ── Config API ────────────────────────────────────────────────────────────

export async function apiFetchConfig(): Promise<FullConfig> {
  return apiFetch<FullConfig>('/admin/api/config');
}

export async function apiSaveConfig(config: Record<string, unknown>): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>('/admin/api/config', {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

// ── Logs ──────────────────────────────────────────────────────────────────

export async function apiFetchLogs(limit?: number): Promise<RequestLog[]> {
  const path = limit ? `/admin/api/logs?limit=${limit}` : '/admin/api/logs';
  return apiFetch<RequestLog[]>(path);
}

export async function apiFetchRuntimeLogs(offset: number = 0, limit: number = 100): Promise<RuntimeLogsResponse> {
  return apiFetch<RuntimeLogsResponse>(`/admin/api/runtime-logs?offset=${offset}&limit=${limit}`);
}

// ── Status & Stats ────────────────────────────────────────────────────────

export async function apiFetchStatus(): Promise<AdminStatusResponse> {
  return apiFetch<AdminStatusResponse>('/admin/api/status');
}

export async function apiFetchStats(): Promise<StatsSnapshot> {
  return apiFetch<StatsSnapshot>('/admin/api/stats');
}

export async function apiFetchModels(): Promise<ModelListResponse> {
  return apiFetch<ModelListResponse>('/admin/api/models');
}
