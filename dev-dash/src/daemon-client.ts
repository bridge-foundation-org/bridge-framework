/**
 * Bridge Daemon HTTP client — covers the full /api/v1/ REST surface.
 */

const DEFAULT_BASE_URL = "http://127.0.0.1:8787";

// ── Shared helpers ────────────────────────────────────────────────────────────

async function req(
  url: string,
  method: string,
  body?: string
): Promise<Response> {
  return fetch(url, { method, body });
}

async function getJson<T = unknown>(url: string): Promise<T> {
  const r = await req(url, "GET");
  return parseJson<T>(r);
}

async function parseJson<T>(r: Response): Promise<T> {
  const text = await r.text();
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as unknown as T;
  }
}

// ── Types ─────────────────────────────────────────────────────────────────────

export interface HealthInfo {
  status: string;
  version: string;
  app: string;
  mode: string;
  redis: string;
  redis_connections: number;
  services: number;
  traces: number;
  sample_rate: number;
}

export interface TraceEntry {
  id: string;
  method: string;
  path: string;
  status: number;
  duration_ms: number;
  timestamp: number;
}

export interface MetricsEndpoint {
  endpoint: string;
  requests: number;
  errors: number;
  avg_ms: number;
}

export interface MetricsInfo {
  total_requests: number;
  total_errors: number;
  endpoints: MetricsEndpoint[];
}

export interface MiddlewareEntry {
  name: string;
  scope: string;
  before: boolean;
  after: boolean;
}

export interface RateLimitRule {
  method: string;
  path: string;
  capacity: number;
  refill_rate: number;
  remaining: number;
}

export interface WatchedFile {
  path: string;
  status: "ok" | "error" | "pending";
  changes: number;
  error?: string;
}

export interface WatchStatus {
  watching: boolean;
  dirs: number;
  files: WatchedFile[];
  sse_clients: number;
  poll_ms: number;
  events_total: number;
}

export interface ConfigSummary {
  app: string;
  version: string;
  mode: string;
  middleware: string[];
  ratelimit: RateLimitRule[];
  watch: { enabled: boolean; poll_ms: number; files: string[] };
}

// ── Client factory ────────────────────────────────────────────────────────────

export function createDaemonClient(url = DEFAULT_BASE_URL) {
  const v1 = `${url}/api/v1`;

  return {
    // ── Core ─────────────────────────────────────────────────────────────────

    async health(): Promise<HealthInfo> {
      return getJson<HealthInfo>(`${v1}/health`);
    },

    async version(): Promise<{ version: string }> {
      return getJson(`${v1}/version`);
    },

    async modeGet(): Promise<{ mode: string }> {
      return getJson(`${v1}/mode`);
    },

    async modeSet(mode: string): Promise<{ mode: string }> {
      const r = await req(`${v1}/mode`, "POST", mode);
      return parseJson(r);
    },

    // ── Compiler ──────────────────────────────────────────────────────────────

    async compile(source: string): Promise<string> {
      const r = await req(`${v1}/compile`, "POST", source);
      if (!r.ok) throw new Error(await r.text());
      return r.text();
    },

    async services(): Promise<Array<{ name: string; auth: string; endpoints: number }>> {
      return getJson(`${v1}/services`);
    },

    async routes(): Promise<Array<{ service: string; name: string; method: string; path: string }>> {
      return getJson(`${v1}/routes`);
    },

    async codegenLatest(): Promise<string> {
      const r = await req(`${v1}/codegen/latest`, "GET");
      if (!r.ok) throw new Error(await r.text());
      return r.text();
    },

    /** Legacy alias */
    async latest(): Promise<string> {
      return this.codegenLatest();
    },

    // ── Auth ──────────────────────────────────────────────────────────────────

    async authStatus(): Promise<{ configured: boolean; scheme: string }> {
      return getJson(`${v1}/auth/status`);
    },

    async authSet(token: string, scheme = "bearer"): Promise<{ message: string; scheme: string }> {
      const r = await req(`${v1}/auth/set`, "POST", JSON.stringify({ scheme, token }));
      return parseJson(r);
    },

    async authClear(): Promise<{ message: string }> {
      const r = await req(`${v1}/auth/clear`, "DELETE");
      return parseJson(r);
    },

    // ── Traces ────────────────────────────────────────────────────────────────

    async tracesList(limit?: number): Promise<TraceEntry[]> {
      const qs = limit !== undefined ? `?limit=${limit}` : "";
      return getJson(`${v1}/traces${qs}`);
    },

    async tracesGet(id: string): Promise<TraceEntry> {
      return getJson(`${v1}/traces/${id}`);
    },

    async tracesClear(): Promise<{ message: string }> {
      const r = await req(`${v1}/traces`, "DELETE");
      return parseJson(r);
    },

    // ── Metrics ───────────────────────────────────────────────────────────────

    async metrics(): Promise<MetricsInfo> {
      return getJson(`${v1}/metrics`);
    },

    async metricsPrometheus(): Promise<string> {
      const r = await req(`${v1}/metrics/prometheus`, "GET");
      return r.text();
    },

    async metricsClear(): Promise<{ message: string }> {
      const r = await req(`${v1}/metrics`, "DELETE");
      return parseJson(r);
    },

    // ── Sampling ──────────────────────────────────────────────────────────────

    async setSamplingRate(rate: number): Promise<{ sample_rate: number }> {
      const r = await req(`${v1}/sampling`, "POST", String(rate));
      return parseJson(r);
    },

    // ── OpenAPI ───────────────────────────────────────────────────────────────

    async openapi(): Promise<unknown> {
      return getJson(`${v1}/openapi`);
    },

    // ── Middleware ────────────────────────────────────────────────────────────

    async middlewareList(): Promise<MiddlewareEntry[]> {
      return getJson(`${v1}/middleware`);
    },

    async middlewareRegister(entry: {
      name: string;
      scope: string;
      before?: string;
      after?: string;
    }): Promise<{ message: string; name: string; index: number }> {
      const r = await req(`${v1}/middleware`, "POST", JSON.stringify(entry));
      return parseJson(r);
    },

    async middlewareRemove(name: string): Promise<{ message: string; name: string }> {
      const r = await req(`${v1}/middleware`, "DELETE", JSON.stringify({ name }));
      return parseJson(r);
    },

    // ── Rate Limiting ─────────────────────────────────────────────────────────

    async rateLimitList(): Promise<RateLimitRule[]> {
      return getJson(`${v1}/ratelimit`);
    },

    async rateLimitAdd(rule: {
      method: string;
      path: string;
      capacity: number;
      refill_rate: number;
    }): Promise<{ message: string }> {
      const r = await req(`${v1}/ratelimit`, "POST", JSON.stringify(rule));
      return parseJson(r);
    },

    async rateLimitRemove(method: string, path: string): Promise<{ message: string }> {
      const r = await req(`${v1}/ratelimit`, "DELETE", JSON.stringify({ method, path }));
      return parseJson(r);
    },

    // ── Hot Reload / Watcher ──────────────────────────────────────────────────

    async watchStatus(): Promise<WatchStatus> {
      return getJson(`${v1}/watch`);
    },

    async watchAddFile(path: string): Promise<{ message: string; path: string }> {
      const r = await req(`${v1}/watch/files`, "POST", path);
      return parseJson(r);
    },

    async watchRemoveFile(path: string): Promise<{ message: string }> {
      const r = await req(`${v1}/watch/files`, "DELETE", JSON.stringify({ path }));
      return parseJson(r);
    },

    async watchAddDir(dir: string): Promise<{ message: string; dir: string; new_files: number }> {
      const r = await req(`${v1}/watch/dirs`, "POST", dir);
      return parseJson(r);
    },

    /** Open a live SSE stream for hot-reload events. */
    watchEvents(): EventSource {
      return new EventSource(`${v1}/watch/events`);
    },

    // ── Config ────────────────────────────────────────────────────────────────

    async config(): Promise<ConfigSummary> {
      return getJson(`${v1}/config`);
    },

    // ── Database (Docker Postgres) ────────────────────────────────────────────

    async dbCreate(name: string): Promise<string> {
      const r = await req(`${v1}/pg/create`, "POST", name);
      return r.text();
    },

    async dbStatus(): Promise<string> {
      const r = await req(`${v1}/pg/status`, "GET");
      return r.text();
    },

    async dbMigrate(sql: string): Promise<string> {
      const r = await req(`${v1}/pg/migrate`, "POST", sql);
      return r.text();
    },

    async dbDestroy(name: string): Promise<string> {
      const r = await req(`${v1}/pg/destroy`, "DELETE", name);
      return r.text();
    },

    // ── Redis ─────────────────────────────────────────────────────────────────

    async redisStatus(): Promise<{ addr: string; connections: number }> {
      return getJson(`${v1}/redis/status`);
    },
  };
}

export type DaemonClient = ReturnType<typeof createDaemonClient>;
