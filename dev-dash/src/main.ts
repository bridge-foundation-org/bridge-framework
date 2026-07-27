import "./style.css";
import { createDaemonClient } from "./daemon-client";
import { docPages, renderMarkdown } from "./docs";
import { 
  renderTracesView, 
  renderMetricsView, 
  renderServicesView,
  updateTraces,
  updateMetrics,
  updateServices,
  fetchTraces,
  fetchMetrics,
  fetchServices
} from "./components";

// ── Configuration ──────────────────────────────────────────────

const BASE_URL = "http://127.0.0.1:8787";
const client = createDaemonClient(BASE_URL);

// ── State ──────────────────────────────────────────────────────

type View = "overview" | "traces" | "metrics" | "services" | "api" | "infrastructure" | "config" | "docs";

interface AppState {
  activeView: View;
  activeDocId: string;
  daemonOnline: boolean;
  dockerAvailable: boolean;
  redisOnline: boolean;
  endpointCount: number;
  middlewareCount: number;
  rateLimitCount: number;
  watchFileCount: number;
}

const state: AppState = {
  activeView: "overview",
  activeDocId: docPages[0]?.id ?? "index",
  daemonOnline: false,
  dockerAvailable: false,
  redisOnline: false,
  endpointCount: 0,
  middlewareCount: 0,
  rateLimitCount: 0,
  watchFileCount: 0,
};

let watchEventSource: EventSource | null = null;

// ── Root Mount Point ───────────────────────────────────────────

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Missing #app element");

// ── Header ─────────────────────────────────────────────────────

function renderHeader(): string {
  const tabs: { id: View; label: string; icon: string }[] = [
    { id: "overview",        label: "Overview",        icon: "⚡" },
    { id: "traces",          label: "Traces",          icon: "⏱️" },
    { id: "metrics",         label: "Metrics",         icon: "📊" },
    { id: "services",        label: "Services",        icon: "🎯" },
    { id: "api",             label: "API Explorer",    icon: "🔌" },
    { id: "infrastructure",  label: "Infrastructure",  icon: "🗄️" },
    { id: "config",          label: "Config",          icon: "⚙️" },
    { id: "docs",            label: "Docs",            icon: "📖" },
  ];

  const navTabs = tabs
    .map(
      (t) =>
        `<button data-view="${t.id}" class="encore-nav-tab ${state.activeView === t.id ? "active" : ""}">${t.icon} ${t.label}</button>`
    )
    .join("");

  return `
    <header class="encore-header">
      <div class="encore-header-inner">
        <div class="encore-logo-area">
          <div class="encore-logo-icon">B</div>
          <div>
            <span class="encore-logo-text">Bridge</span>
            <span class="encore-logo-badge">Local Dev</span>
          </div>
        </div>
        <nav class="encore-nav">${navTabs}</nav>
        <div class="encore-status-bar">
          <div class="encore-status-pill">
            <span class="encore-status-dot ${state.daemonOnline ? "online" : "offline"}"></span>
            <span>Daemon</span>
          </div>
          <div class="encore-status-pill">
            <span class="encore-status-dot ${state.redisOnline ? "online" : "offline"}"></span>
            <span>Redis</span>
          </div>
        </div>
      </div>
    </header>`;
}

function shell(content: string): string {
  return `
    <div class="encore-shell">
      ${renderHeader()}
      <main class="encore-main encore-fade-in">${content}</main>
    </div>`;
}

// ── Helpers ────────────────────────────────────────────────────

function showOutput(value: string, type?: "success" | "error" | "accent") {
  const output = document.querySelector<HTMLPreElement>("#output");
  if (output) {
    output.textContent = value;
    output.className = "encore-output" + (type ? ` ${type}` : "");
  }
}

function showToast(message: string, icon = "✓") {
  const existing = document.querySelector(".encore-toast");
  if (existing) existing.remove();
  const toast = document.createElement("div");
  toast.className = "encore-toast";
  toast.innerHTML = `<span>${icon}</span> ${message}`;
  document.body.appendChild(toast);
  setTimeout(() => toast.remove(), 3000);
}

function fmtJson(v: unknown): string {
  try { return JSON.stringify(v, null, 2); }
  catch { return String(v); }
}

// ── Source Endpoint Parser ─────────────────────────────────────

interface ParsedEndpoint { service: string; name: string; method: string; path: string; }

function parseSourceEndpoints(source: string): ParsedEndpoint[] {
  const lines = source.split("\n").map((l) => l.trim()).filter(Boolean);
  let svc = "unknown";
  const eps: ParsedEndpoint[] = [];
  for (const line of lines) {
    if (line.startsWith("service ")) { svc = line.slice(8).trim(); }
    else if (line.startsWith("endpoint ")) {
      const parts = line.slice(9).trim().split(/\s+/);
      if (parts.length >= 3) eps.push({ service: svc, name: parts[0], method: parts[1], path: parts[2] });
    }
  }
  return eps;
}


// ── Overview View ──────────────────────────────────────────────

function renderOverview(): string {
  return shell(`
    <div class="encore-stats" style="margin-bottom:24px;">
      <div class="encore-stat">
        <div class="encore-stat-value" id="statEndpoints">${state.endpointCount}</div>
        <div class="encore-stat-label">Endpoints</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value">${state.middlewareCount}</div>
        <div class="encore-stat-label">Middleware</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value">${state.rateLimitCount}</div>
        <div class="encore-stat-label">Rate Rules</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value"><span class="encore-status-dot ${state.daemonOnline ? "online" : "offline"}" style="display:inline-block;vertical-align:middle;"></span></div>
        <div class="encore-stat-label">Daemon</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value"><span class="encore-status-dot ${state.redisOnline ? "online" : "offline"}" style="display:inline-block;vertical-align:middle;"></span></div>
        <div class="encore-stat-label">Redis</div>
      </div>
    </div>

    <div class="encore-card" style="margin-bottom:24px;">
      <div class="encore-card-header">
        <div>
          <div class="encore-card-title">
            <div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">⚡</div>
            Bridge Flow
          </div>
          <div class="encore-card-subtitle">Architecture overview</div>
        </div>
        <span class="encore-tag accent">Live</span>
      </div>
      <div class="encore-flow">
        <div class="encore-flow-node compiler-node"><div class="node-icon">📝</div><div class="node-title">.bridge DSL</div><div class="node-subtitle">Source file</div></div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node service-node"><div class="node-icon">⚙️</div><div class="node-title">Compiler</div><div class="node-subtitle">Parse + validate</div></div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node service-node"><div class="node-icon">🔧</div><div class="node-title">Codegen</div><div class="node-subtitle">TypeScript client</div></div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node db-node"><div class="node-icon">🐘</div><div class="node-title">PostgreSQL</div><div class="node-subtitle">Docker container</div></div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node redis-node"><div class="node-icon">⚡</div><div class="node-title">Miniredis</div><div class="node-subtitle">Cache layer</div></div>
      </div>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-success-dim);color:var(--encore-success);">⚙️</div>Compiler</div>
            <div class="encore-card-subtitle">Compile Bridge DSL to TypeScript clients</div>
          </div>
        </div>
        <textarea id="source" class="encore-textarea" rows="6" placeholder="service hello&#10;endpoint ping GET /ping">service hello
endpoint ping GET /ping
endpoint echo POST /echo</textarea>
        <div class="encore-btn-group" style="margin-top:12px;">
          <button id="compile" class="encore-btn encore-btn-primary">⚡ Compile</button>
          <button id="latest" class="encore-btn">📦 Load Latest</button>
          <button id="parseEndpoints" class="encore-btn">🔍 Parse Endpoints</button>
        </div>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">🎛️</div>Daemon Controls</div>
            <div class="encore-card-subtitle">Backend at <code class="encore-code-inline">${BASE_URL}</code></div>
          </div>
          <span class="encore-tag ${state.daemonOnline ? "success" : "error"}">${state.daemonOnline ? "● Online" : "● Offline"}</span>
        </div>
        <div class="encore-btn-group" style="margin-bottom:12px;">
          <button id="health" class="encore-btn encore-btn-success">❤️ Health</button>
          <button id="modeGet" class="encore-btn">📋 Get Mode</button>
        </div>
        <div style="display:flex;gap:8px;align-items:end;">
          <div style="flex:1;">
            <label class="encore-label">Daemon Mode</label>
            <input id="modeValue" value="full" class="encore-input" placeholder="lite|full|ultra|off" />
          </div>
          <button id="modeSet" class="encore-btn encore-btn-primary">Set Mode</button>
        </div>
      </div>

      <div class="encore-card encore-grid-full">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-info-dim);color:var(--encore-info);">📡</div>Service Catalog</div>
            <div class="encore-card-subtitle">Parsed endpoints from your Bridge source</div>
          </div>
          <span class="encore-tag info" id="endpointCountBadge">0 endpoints</span>
        </div>
        <div style="overflow-x:auto;">
          <table class="encore-table">
            <thead><tr><th>Service</th><th>Endpoint</th><th>Method</th><th>Path</th></tr></thead>
            <tbody id="endpointBody"><tr><td colspan="4" class="encore-empty-state">Click "Parse Endpoints" to see your service routes</td></tr></tbody>
          </table>
        </div>
      </div>
    </div>

    <div class="encore-card" style="margin-top:20px;">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>Output</div>
      </div>
      <pre id="output" class="encore-output">Ready.</pre>
    </div>
  `);
}

// ── API Explorer View ──────────────────────────────────────────

function renderApiExplorer(): string {
  const endpoints = [
    "GET /api/v1/health", "GET /api/v1/version", "GET /api/v1/mode", "POST /api/v1/mode",
    "POST /api/v1/compile", "GET /api/v1/services", "GET /api/v1/routes",
    "GET /api/v1/codegen/latest",
    "GET /api/v1/auth/status", "POST /api/v1/auth/set", "DELETE /api/v1/auth/clear",
    "GET /api/v1/traces", "DELETE /api/v1/traces",
    "GET /api/v1/metrics", "GET /api/v1/metrics/prometheus", "DELETE /api/v1/metrics",
    "POST /api/v1/sampling", "GET /api/v1/openapi",
    "GET /api/v1/middleware", "POST /api/v1/middleware", "DELETE /api/v1/middleware",
    "GET /api/v1/ratelimit", "POST /api/v1/ratelimit", "DELETE /api/v1/ratelimit",
    "GET /api/v1/watch", "POST /api/v1/watch/files", "DELETE /api/v1/watch/files", "POST /api/v1/watch/dirs",
    "GET /api/v1/config",
    "GET /api/v1/pg/status", "POST /api/v1/pg/create", "POST /api/v1/pg/migrate", "DELETE /api/v1/pg/destroy",
    "GET /api/v1/redis/status",
  ];

  return shell(`
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">API Explorer</div>
        <div class="encore-section-subtitle">Test daemon HTTP endpoints interactively</div>
      </div>
    </div>
    <div class="encore-grid encore-grid-2 encore-stagger">
      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">🔌</div>Request</div>
        </div>
        <div style="margin-bottom:12px;">
          <label class="encore-label">Endpoint</label>
          <select id="apiEndpoint" class="encore-select" style="width:100%;">
            ${endpoints.map((e) => `<option value="${e}">${e}</option>`).join("")}
          </select>
        </div>
        <div style="margin-bottom:12px;">
          <label class="encore-label">Request Body</label>
          <textarea id="apiBody" class="encore-textarea" rows="5" placeholder="Request body (JSON or plain text)"></textarea>
        </div>
        <button id="apiSend" class="encore-btn encore-btn-primary" style="width:100%;">▶ Send Request</button>
      </div>
      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-success-dim);color:var(--encore-success);">📨</div>Response</div>
          <span class="encore-tag" id="apiStatusTag" style="display:none;"></span>
        </div>
        <pre id="apiResponse" class="encore-output" style="min-height:200px;">Response will appear here.</pre>
      </div>
    </div>
  `);
}


// ── Infrastructure View ────────────────────────────────────────

function renderInfrastructure(): string {
  return shell(`
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Infrastructure</div>
        <div class="encore-section-subtitle">Manage databases, caching, middleware, rate limiting, and hot reload</div>
      </div>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">

      <!-- Database Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-postgres-dim);color:var(--encore-postgres);">🐘</div>PostgreSQL</div>
            <div class="encore-card-subtitle">Docker container management</div>
          </div>
          <span class="encore-tag ${state.dockerAvailable ? "success" : "warning"}">${state.dockerAvailable ? "Docker ✓" : "Docker ?"}</span>
        </div>
        <div style="margin-bottom:12px;">
          <label class="encore-label">Container Name</label>
          <input id="dbName" value="default" class="encore-input" placeholder="e.g. myapp" />
        </div>
        <div class="encore-btn-group" style="margin-bottom:16px;">
          <button id="dbCreate" class="encore-btn encore-btn-success">＋ Create</button>
          <button id="dbStatus" class="encore-btn">📊 Status</button>
          <button id="dbDestroy" class="encore-btn encore-btn-danger">✕ Destroy</button>
        </div>
        <hr class="encore-divider" />
        <label class="encore-label">SQL Migration</label>
        <textarea id="migrateSql" class="encore-textarea" rows="3" placeholder="CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT);"></textarea>
        <button id="dbMigrate" class="encore-btn encore-btn-primary" style="width:100%;margin-top:8px;">▶ Run Migration</button>
      </div>

      <!-- Redis Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-redis-dim);color:var(--encore-redis);">⚡</div>Miniredis</div>
            <div class="encore-card-subtitle">Embedded Redis-compatible cache server</div>
          </div>
          <span class="encore-tag ${state.redisOnline ? "success" : "error"}">${state.redisOnline ? "● Running" : "● Stopped"}</span>
        </div>
        <div class="encore-stats" style="margin-bottom:16px;">
          <div class="encore-stat"><div class="encore-stat-value" id="redisAddr">—</div><div class="encore-stat-label">Address</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="redisConns">—</div><div class="encore-stat-label">Connections</div></div>
        </div>
        <button id="redisStatus" class="encore-btn" style="width:100%;margin-bottom:12px;">🔄 Refresh Status</button>
        <pre id="redisOutput" class="encore-output" style="min-height:60px;">Click Refresh Status.</pre>
      </div>

      <!-- Middleware Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">🔗</div>Middleware</div>
            <div class="encore-card-subtitle">Composable before/after request hooks</div>
          </div>
          <button id="mwRefresh" class="encore-btn" style="font-size:12px;padding:4px 10px;">🔄</button>
        </div>
        <div style="overflow-x:auto;margin-bottom:12px;">
          <table class="encore-table">
            <thead><tr><th>Name</th><th>Scope</th><th>Before</th><th>After</th></tr></thead>
            <tbody id="mwBody"><tr><td colspan="4" class="encore-empty-state">Click 🔄 to load</td></tr></tbody>
          </table>
        </div>
        <hr class="encore-divider" />
        <div class="encore-grid encore-grid-2" style="gap:8px;margin-bottom:8px;">
          <div>
            <label class="encore-label">Name</label>
            <input id="mwName" class="encore-input" placeholder="my-middleware" />
          </div>
          <div>
            <label class="encore-label">Scope</label>
            <input id="mwScope" class="encore-input" placeholder="global | service:users | GET:/ping" />
          </div>
          <div>
            <label class="encore-label">Before hook</label>
            <select id="mwBefore" class="encore-select" style="width:100%;">
              <option value="">— none —</option>
              <option value="log">log</option>
              <option value="reject:403:forbidden">reject:403:forbidden</option>
              <option value="reject:401:unauthenticated">reject:401:unauthenticated</option>
            </select>
          </div>
          <div>
            <label class="encore-label">After hook</label>
            <select id="mwAfter" class="encore-select" style="width:100%;">
              <option value="">— none —</option>
              <option value="log">log</option>
              <option value="header:X-Powered-By:bridge">header:X-Powered-By:bridge</option>
              <option value="header:X-Custom:value">header:X-Custom:value</option>
            </select>
          </div>
        </div>
        <div class="encore-btn-group">
          <button id="mwRegister" class="encore-btn encore-btn-primary">＋ Register</button>
          <button id="mwRemove" class="encore-btn encore-btn-danger">✕ Remove</button>
        </div>
      </div>

      <!-- Rate Limiting Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-warning-dim,#3d2e00);color:var(--encore-warning,#f59e0b);">🚦</div>Rate Limiting</div>
            <div class="encore-card-subtitle">Token-bucket throttling per endpoint</div>
          </div>
          <button id="rlRefresh" class="encore-btn" style="font-size:12px;padding:4px 10px;">🔄</button>
        </div>
        <div style="overflow-x:auto;margin-bottom:12px;">
          <table class="encore-table">
            <thead><tr><th>Method</th><th>Path</th><th>Cap</th><th>Rate/s</th><th>Left</th></tr></thead>
            <tbody id="rlBody"><tr><td colspan="5" class="encore-empty-state">Click 🔄 to load</td></tr></tbody>
          </table>
        </div>
        <hr class="encore-divider" />
        <div class="encore-grid encore-grid-2" style="gap:8px;margin-bottom:8px;">
          <div>
            <label class="encore-label">Method</label>
            <input id="rlMethod" class="encore-input" value="*" placeholder="GET | POST | *" />
          </div>
          <div>
            <label class="encore-label">Path</label>
            <input id="rlPath" class="encore-input" value="*" placeholder="/api/v1/compile | *" />
          </div>
          <div>
            <label class="encore-label">Capacity</label>
            <input id="rlCapacity" class="encore-input" type="number" value="100" min="1" />
          </div>
          <div>
            <label class="encore-label">Refill/sec</label>
            <input id="rlRefillRate" class="encore-input" type="number" value="10" min="0.1" step="0.1" />
          </div>
        </div>
        <div class="encore-btn-group">
          <button id="rlAdd" class="encore-btn encore-btn-primary">＋ Add Rule</button>
          <button id="rlRemove" class="encore-btn encore-btn-danger">✕ Remove</button>
        </div>
      </div>

      <!-- Hot Reload Panel (full width) -->
      <div class="encore-card encore-grid-full">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-success-dim);color:var(--encore-success);">🔄</div>Hot Reload</div>
            <div class="encore-card-subtitle">Watch .bridge files — auto-recompile on change</div>
          </div>
          <button id="watchRefresh" class="encore-btn" style="font-size:12px;padding:4px 10px;">🔄 Refresh</button>
        </div>
        <div class="encore-stats" style="margin-bottom:16px;">
          <div class="encore-stat"><div class="encore-stat-value" id="watchActive">—</div><div class="encore-stat-label">Watching</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="watchFiles">${state.watchFileCount}</div><div class="encore-stat-label">Files</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="watchEvents">—</div><div class="encore-stat-label">Events</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="watchPollMs">—</div><div class="encore-stat-label">Poll ms</div></div>
        </div>
        <div class="encore-grid encore-grid-2" style="gap:12px;margin-bottom:12px;">
          <div>
            <label class="encore-label">Watch File (.bridge)</label>
            <div style="display:flex;gap:8px;">
              <input id="watchFilePath" class="encore-input" style="flex:1;" placeholder="app.bridge" />
              <button id="watchAddFile" class="encore-btn encore-btn-primary">＋ Watch</button>
            </div>
          </div>
          <div>
            <label class="encore-label">Watch Directory</label>
            <div style="display:flex;gap:8px;">
              <input id="watchDirPath" class="encore-input" style="flex:1;" placeholder="." />
              <button id="watchAddDir" class="encore-btn encore-btn-primary">＋ Dir</button>
            </div>
          </div>
        </div>
        <div style="margin-bottom:12px;">
          <div id="watchFileList" style="display:flex;flex-wrap:wrap;gap:8px;min-height:32px;">
            <span style="color:var(--encore-text-dim);font-size:13px;">No files watched yet.</span>
          </div>
        </div>
        <div style="display:flex;gap:8px;align-items:center;margin-bottom:8px;">
          <button id="watchConnect" class="encore-btn encore-btn-success">📡 Connect SSE</button>
          <button id="watchDisconnect" class="encore-btn encore-btn-danger" style="display:none;">⏹ Disconnect</button>
          <span id="sseBadge" class="encore-tag" style="display:none;"></span>
        </div>
        <pre id="watchEventLog" class="encore-output" style="min-height:80px;max-height:200px;overflow-y:auto;">SSE events will appear here after connecting.</pre>
      </div>
    </div>

    <div class="encore-card" style="margin-top:20px;">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>Infrastructure Output</div>
      </div>
      <pre id="output" class="encore-output">Infrastructure output will appear here.</pre>
    </div>
  `);
}


// ── Config View ────────────────────────────────────────────────

function renderConfig(): string {
  const sampleToml = `[project]
name    = "my-app"
version = "0.1.0"

[daemon]
http_addr  = "127.0.0.1:8787"
tcp_addr   = "127.0.0.1:7878"
redis_addr = "127.0.0.1:6399"
mode       = "full"

[watch]
enabled = true
poll_ms = 500
dirs    = ["."]
files   = ["app.bridge"]

[[middleware.rules]]
name   = "powered-by"
scope  = "global"
after  = "header:X-Powered-By:bridge"

[[ratelimit.rules]]
method      = "POST"
path        = "/api/v1/compile"
capacity    = 60
refill_rate = 1.0`;

  return shell(`
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Project Config</div>
        <div class="encore-section-subtitle">Runtime configuration from bridge.toml</div>
      </div>
      <button id="configRefresh" class="encore-btn">🔄 Refresh</button>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">

      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">⚙️</div>Runtime Config</div>
            <div class="encore-card-subtitle">Live values from GET /api/v1/config</div>
          </div>
        </div>
        <div class="encore-stats" style="margin-bottom:16px;">
          <div class="encore-stat"><div class="encore-stat-value" id="cfgApp">—</div><div class="encore-stat-label">App</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="cfgVersion">—</div><div class="encore-stat-label">Version</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="cfgMode">—</div><div class="encore-stat-label">Mode</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="cfgMw">—</div><div class="encore-stat-label">Middleware</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="cfgRl">—</div><div class="encore-stat-label">Rate Rules</div></div>
          <div class="encore-stat"><div class="encore-stat-value" id="cfgWatch">—</div><div class="encore-stat-label">Watching</div></div>
        </div>
        <pre id="cfgRaw" class="encore-output" style="max-height:260px;overflow-y:auto;">Click Refresh to load.</pre>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📄</div>bridge.toml Reference</div>
            <div class="encore-card-subtitle">Default configuration template</div>
          </div>
        </div>
        <pre class="encore-output" style="max-height:360px;overflow-y:auto;font-size:12px;">${sampleToml}</pre>
        <div style="margin-top:12px;font-size:13px;color:var(--encore-text-dim);">
          <p>Place <code class="encore-code-inline">bridge.toml</code> in your project root. The daemon reads it at startup.</p>
          <p style="margin-top:6px;">Override with: <code class="encore-code-inline">BRIDGE_CONFIG=/path/to/config.toml</code></p>
          <p style="margin-top:6px;">Generated by: <code class="encore-code-inline">bridge init &lt;dir&gt;</code></p>
        </div>
      </div>

      <div class="encore-card encore-grid-full">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>Section Reference</div>
        </div>
        <table class="encore-table">
          <thead><tr><th>Section</th><th>Key</th><th>Type</th><th>Default</th><th>Description</th></tr></thead>
          <tbody>
            <tr><td><code class="encore-code-inline">[project]</code></td><td>name</td><td>string</td><td>""</td><td>Project name shown in dashboard and logs</td></tr>
            <tr><td><code class="encore-code-inline">[project]</code></td><td>version</td><td>string</td><td>"0.1.0"</td><td>Project version</td></tr>
            <tr><td><code class="encore-code-inline">[daemon]</code></td><td>http_addr</td><td>string</td><td>"127.0.0.1:8787"</td><td>HTTP server bind address</td></tr>
            <tr><td><code class="encore-code-inline">[daemon]</code></td><td>tcp_addr</td><td>string</td><td>"127.0.0.1:7878"</td><td>TCP protocol server address</td></tr>
            <tr><td><code class="encore-code-inline">[daemon]</code></td><td>redis_addr</td><td>string</td><td>"127.0.0.1:6399"</td><td>Miniredis bind address</td></tr>
            <tr><td><code class="encore-code-inline">[daemon]</code></td><td>mode</td><td>string</td><td>"full"</td><td>lite | full | ultra | off</td></tr>
            <tr><td><code class="encore-code-inline">[watch]</code></td><td>enabled</td><td>bool</td><td>true</td><td>Enable hot-reload watcher</td></tr>
            <tr><td><code class="encore-code-inline">[watch]</code></td><td>poll_ms</td><td>integer</td><td>500</td><td>File polling interval (min 100ms)</td></tr>
            <tr><td><code class="encore-code-inline">[watch]</code></td><td>dirs</td><td>array</td><td>[]</td><td>Directories to scan for .bridge files</td></tr>
            <tr><td><code class="encore-code-inline">[watch]</code></td><td>files</td><td>array</td><td>[]</td><td>Explicit .bridge files to watch</td></tr>
            <tr><td><code class="encore-code-inline">[[middleware.rules]]</code></td><td>name</td><td>string</td><td>—</td><td>Unique middleware name</td></tr>
            <tr><td><code class="encore-code-inline">[[middleware.rules]]</code></td><td>scope</td><td>string</td><td>"global"</td><td>global | service:NAME | METHOD:/path</td></tr>
            <tr><td><code class="encore-code-inline">[[middleware.rules]]</code></td><td>before</td><td>string</td><td>null</td><td>log | reject:STATUS:msg</td></tr>
            <tr><td><code class="encore-code-inline">[[middleware.rules]]</code></td><td>after</td><td>string</td><td>null</td><td>log | header:KEY:VALUE</td></tr>
            <tr><td><code class="encore-code-inline">[[ratelimit.rules]]</code></td><td>method</td><td>string</td><td>"*"</td><td>HTTP method or * for wildcard</td></tr>
            <tr><td><code class="encore-code-inline">[[ratelimit.rules]]</code></td><td>path</td><td>string</td><td>"*"</td><td>Exact path or * for wildcard</td></tr>
            <tr><td><code class="encore-code-inline">[[ratelimit.rules]]</code></td><td>capacity</td><td>integer</td><td>—</td><td>Max burst tokens (must be &gt; 0)</td></tr>
            <tr><td><code class="encore-code-inline">[[ratelimit.rules]]</code></td><td>refill_rate</td><td>float</td><td>—</td><td>Tokens refilled per second</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  `);
}

// ── Docs View ──────────────────────────────────────────────────

function renderDocs(): string {
  const sidebarItems = docPages
    .map(
      (page) =>
        `<button data-doc="${page.id}" class="encore-sidebar-item ${state.activeDocId === page.id ? "active" : ""}">${page.title}</button>`
    )
    .join("");
  const page = docPages.find((p) => p.id === state.activeDocId) ?? docPages[0]!;
  return shell(`
    <div class="encore-docs-layout">
      <aside class="encore-card encore-sidebar">
        <div class="encore-sidebar-title">Documentation</div>
        <nav>${sidebarItems}</nav>
      </aside>
      <article class="encore-card encore-slide-in">
        <span class="encore-tag accent" style="margin-bottom:8px;">${page.subtitle}</span>
        <h1 style="font-size:28px;font-weight:800;color:white;margin:8px 0 24px;letter-spacing:-0.5px;">${page.title}</h1>
        <div class="encore-prose">${renderMarkdown(page.body)}</div>
      </article>
    </div>
  `);
}


// ── Mount + Bind Events ────────────────────────────────────────

function mount() {
  switch (state.activeView) {
    case "overview":        app.innerHTML = renderOverview();        break;
    case "traces":          app.innerHTML = renderTracesView();       break;
    case "metrics":         app.innerHTML = renderMetricsView();      break;
    case "services":        app.innerHTML = renderServicesView();     break;
    case "api":             app.innerHTML = renderApiExplorer();     break;
    case "infrastructure":  app.innerHTML = renderInfrastructure();  break;
    case "config":          app.innerHTML = renderConfig();          break;
    case "docs":            app.innerHTML = renderDocs();            break;
  }
  bindNavigation();
  if (state.activeView === "overview")        bindOverviewEvents();
  if (state.activeView === "api")             bindApiExplorerEvents();
  if (state.activeView === "infrastructure")  bindInfrastructureEvents();
  if (state.activeView === "config")          bindConfigEvents();
  
  // New views initialization
  if (state.activeView === "traces") bindTracesEvents();
  if (state.activeView === "metrics") bindMetricsEvents();
  if (state.activeView === "services") bindServicesEvents();
}

function bindNavigation() {
  app.querySelectorAll<HTMLButtonElement>(".encore-nav-tab").forEach((btn) => {
    btn.onclick = () => { state.activeView = (btn.dataset.view as View) ?? "overview"; mount(); };
  });
  app.querySelectorAll<HTMLButtonElement>(".encore-sidebar-item").forEach((btn) => {
    btn.onclick = () => { state.activeDocId = btn.dataset.doc ?? "index"; mount(); };
  });
}

function bindOverviewEvents() {
  const source    = document.querySelector<HTMLTextAreaElement>("#source");
  const modeValue = document.querySelector<HTMLInputElement>("#modeValue");
  if (!source || !modeValue) return;

  document.querySelector<HTMLButtonElement>("#health")!.onclick = async () => {
    try {
      const r = await client.health();
      showOutput(fmtJson(r), "success");
      showToast("Daemon is healthy", "❤️");
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#modeGet")!.onclick = async () => {
    try { showOutput(fmtJson(await client.modeGet())); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#modeSet")!.onclick = async () => {
    try {
      const r = await client.modeSet(modeValue.value);
      showOutput(fmtJson(r), "success");
      showToast(`Mode set to ${modeValue.value}`);
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#compile")!.onclick = async () => {
    try {
      const result = await client.compile(source.value);
      showOutput(result, "accent");
      showToast("Compiled successfully", "⚡");
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#latest")!.onclick = async () => {
    try { showOutput(await client.latest(), "accent"); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#parseEndpoints")!.onclick = () => {
    const endpoints = parseSourceEndpoints(source.value);
    state.endpointCount = endpoints.length;
    const body  = document.querySelector<HTMLTableSectionElement>("#endpointBody")!;
    const badge = document.querySelector<HTMLSpanElement>("#endpointCountBadge");
    const stat  = document.querySelector<HTMLDivElement>("#statEndpoints");
    if (badge) badge.textContent = `${endpoints.length} endpoint${endpoints.length !== 1 ? "s" : ""}`;
    if (stat)  stat.textContent  = String(endpoints.length);
    body.innerHTML = endpoints.length === 0
      ? `<tr><td colspan="4" class="encore-empty-state">No endpoints found.</td></tr>`
      : endpoints.map((ep) => `<tr>
          <td><span class="encore-tag accent">${ep.service}</span></td>
          <td style="font-weight:600;color:white;">${ep.name}</td>
          <td><span class="encore-method-tag ${ep.method.toLowerCase()}">${ep.method}</span></td>
          <td><code class="encore-code-inline">${ep.path}</code></td>
        </tr>`).join("");
    if (endpoints.length) showToast(`Parsed ${endpoints.length} endpoints`, "📡");
  };
}

function bindApiExplorerEvents() {
  const sel  = document.querySelector<HTMLSelectElement>("#apiEndpoint")!;
  const body = document.querySelector<HTMLTextAreaElement>("#apiBody")!;
  const resp = document.querySelector<HTMLPreElement>("#apiResponse")!;
  const tag  = document.querySelector<HTMLSpanElement>("#apiStatusTag")!;

  document.querySelector<HTMLButtonElement>("#apiSend")!.onclick = async () => {
    const [method, ...parts] = sel.value.split(" ");
    const path = parts.join(" ");
    resp.textContent = "Sending…";
    resp.className = "encore-output";
    tag.style.display = "none";
    try {
      const opts: RequestInit = { method };
      if (method !== "GET") opts.body = body.value;
      const r = await fetch(`${BASE_URL}${path}`, opts);
      const text = await r.text();
      let formatted = text;
      try { formatted = JSON.stringify(JSON.parse(text), null, 2); } catch { /* plain */ }
      resp.textContent = `HTTP ${r.status} ${r.statusText}\n\n${formatted}`;
      resp.className = `encore-output ${r.ok ? "success" : "error"}`;
      tag.textContent = `${r.status} ${r.statusText}`;
      tag.className = `encore-tag ${r.ok ? "success" : "error"}`;
      tag.style.display = "inline-flex";
    } catch (err) {
      resp.textContent = `Network Error: ${err}`;
      resp.className = "encore-output error";
    }
  };
}


function bindInfrastructureEvents() {
  const dbName    = document.querySelector<HTMLInputElement>("#dbName")!;
  const migrateSql= document.querySelector<HTMLTextAreaElement>("#migrateSql")!;
  const redisOut  = document.querySelector<HTMLPreElement>("#redisOutput")!;

  // DB
  document.querySelector<HTMLButtonElement>("#dbCreate")!.onclick = async () => {
    try { showOutput(await client.dbCreate(dbName.value || "default"), "success"); showToast("DB created", "🐘"); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  document.querySelector<HTMLButtonElement>("#dbStatus")!.onclick = async () => {
    try { const r = await client.dbStatus(); showOutput(r); state.dockerAvailable = !r.includes("not available"); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  document.querySelector<HTMLButtonElement>("#dbDestroy")!.onclick = async () => {
    try { showOutput(await client.dbDestroy(dbName.value || "default"), "error"); showToast("DB destroyed", "✕"); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  document.querySelector<HTMLButtonElement>("#dbMigrate")!.onclick = async () => {
    try { showOutput(await client.dbMigrate(migrateSql.value), "success"); showToast("Migration done", "▶"); }
    catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  // Redis
  document.querySelector<HTMLButtonElement>("#redisStatus")!.onclick = async () => {
    try {
      const r = await client.redisStatus();
      redisOut.textContent = fmtJson(r);
      const addrEl  = document.querySelector<HTMLDivElement>("#redisAddr");
      const connsEl = document.querySelector<HTMLDivElement>("#redisConns");
      if (addrEl)  addrEl.textContent  = r.addr ?? "—";
      if (connsEl) connsEl.textContent = String(r.connections ?? "—");
      state.redisOnline = r.addr !== "not running";
      showToast("Redis refreshed", "⚡");
    } catch (e) { redisOut.textContent = `Error: ${e}`; }
  };

  // Middleware
  async function refreshMw() {
    try {
      const list = await client.middlewareList();
      state.middlewareCount = list.length;
      const tbody = document.querySelector<HTMLTableSectionElement>("#mwBody")!;
      tbody.innerHTML = list.length === 0
        ? `<tr><td colspan="4" class="encore-empty-state">No middleware registered.</td></tr>`
        : list.map((m) => `<tr>
            <td style="font-weight:600;color:white;">${m.name}</td>
            <td><code class="encore-code-inline">${m.scope}</code></td>
            <td>${m.before ? "✓" : "—"}</td>
            <td>${m.after ? "✓" : "—"}</td>
          </tr>`).join("");
    } catch (e) { showOutput(`Middleware error: ${e}`, "error"); }
  }

  document.querySelector<HTMLButtonElement>("#mwRefresh")!.onclick = refreshMw;
  document.querySelector<HTMLButtonElement>("#mwRegister")!.onclick = async () => {
    const name   = (document.querySelector<HTMLInputElement>("#mwName")!).value.trim();
    const scope  = (document.querySelector<HTMLInputElement>("#mwScope")!).value.trim() || "global";
    const before = (document.querySelector<HTMLSelectElement>("#mwBefore")!).value || undefined;
    const after  = (document.querySelector<HTMLSelectElement>("#mwAfter")!).value  || undefined;
    if (!name) { showOutput("Middleware name is required", "error"); return; }
    try {
      const r = await client.middlewareRegister({ name, scope, before, after });
      showOutput(fmtJson(r), "success");
      showToast(`Middleware "${name}" registered`);
      refreshMw();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  document.querySelector<HTMLButtonElement>("#mwRemove")!.onclick = async () => {
    const name = (document.querySelector<HTMLInputElement>("#mwName")!).value.trim();
    if (!name) { showOutput("Enter a name to remove", "error"); return; }
    try {
      const r = await client.middlewareRemove(name);
      showOutput(fmtJson(r), "success");
      showToast(`Middleware "${name}" removed`, "✕");
      refreshMw();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  refreshMw();

  // Rate Limiting
  async function refreshRl() {
    try {
      const rules = await client.rateLimitList();
      state.rateLimitCount = rules.length;
      const tbody = document.querySelector<HTMLTableSectionElement>("#rlBody")!;
      tbody.innerHTML = rules.length === 0
        ? `<tr><td colspan="5" class="encore-empty-state">No rate-limit rules.</td></tr>`
        : rules.map((r) => `<tr>
            <td><span class="encore-method-tag ${r.method === "*" ? "get" : r.method.toLowerCase()}">${r.method}</span></td>
            <td><code class="encore-code-inline">${r.path}</code></td>
            <td>${r.capacity}</td>
            <td>${r.refill_rate}</td>
            <td>${r.remaining}</td>
          </tr>`).join("");
    } catch (e) { showOutput(`Rate limit error: ${e}`, "error"); }
  }

  document.querySelector<HTMLButtonElement>("#rlRefresh")!.onclick = refreshRl;
  document.querySelector<HTMLButtonElement>("#rlAdd")!.onclick = async () => {
    const method      = (document.querySelector<HTMLInputElement>("#rlMethod")!).value.trim()      || "*";
    const path        = (document.querySelector<HTMLInputElement>("#rlPath")!).value.trim()        || "*";
    const capacity    = parseInt((document.querySelector<HTMLInputElement>("#rlCapacity")!).value, 10);
    const refill_rate = parseFloat((document.querySelector<HTMLInputElement>("#rlRefillRate")!).value);
    if (!capacity || !refill_rate) { showOutput("Capacity and refill_rate are required", "error"); return; }
    try {
      const r = await client.rateLimitAdd({ method, path, capacity, refill_rate });
      showOutput(fmtJson(r), "success");
      showToast("Rate limit rule added");
      refreshRl();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  document.querySelector<HTMLButtonElement>("#rlRemove")!.onclick = async () => {
    const method = (document.querySelector<HTMLInputElement>("#rlMethod")!).value.trim() || "*";
    const path   = (document.querySelector<HTMLInputElement>("#rlPath")!).value.trim()   || "*";
    try {
      const r = await client.rateLimitRemove(method, path);
      showOutput(fmtJson(r), "success");
      showToast("Rate limit rule removed", "✕");
      refreshRl();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };
  refreshRl();

  // Hot Reload
  async function refreshWatch() {
    try {
      const ws = await client.watchStatus();
      state.watchFileCount = ws.files.length;
      const activeEl  = document.querySelector<HTMLDivElement>("#watchActive");
      const filesEl   = document.querySelector<HTMLDivElement>("#watchFiles");
      const eventsEl  = document.querySelector<HTMLDivElement>("#watchEvents");
      const pollEl    = document.querySelector<HTMLDivElement>("#watchPollMs");
      if (activeEl)  activeEl.textContent  = ws.watching ? "✓ yes" : "no";
      if (filesEl)   filesEl.textContent   = String(ws.files.length);
      if (eventsEl)  eventsEl.textContent  = String(ws.events_total);
      if (pollEl)    pollEl.textContent    = String(ws.poll_ms);
      const listEl = document.querySelector<HTMLDivElement>("#watchFileList");
      if (listEl) {
        listEl.innerHTML = ws.files.length === 0
          ? `<span style="color:var(--encore-text-dim);font-size:13px;">No files watched.</span>`
          : ws.files.map((f) => {
              const color = f.status === "ok" ? "success" : f.status === "error" ? "error" : "accent";
              return `<span class="encore-tag ${color}" title="${f.error ?? ""}">${f.path.split(/[\\/]/).pop()} (${f.status}, ${f.changes}×)</span>`;
            }).join("");
      }
    } catch (e) { /* daemon may be offline */ }
  }

  document.querySelector<HTMLButtonElement>("#watchRefresh")!.onclick = refreshWatch;

  document.querySelector<HTMLButtonElement>("#watchAddFile")!.onclick = async () => {
    const p = (document.querySelector<HTMLInputElement>("#watchFilePath")!).value.trim();
    if (!p) { showOutput("Enter a file path", "error"); return; }
    try {
      const r = await client.watchAddFile(p);
      showOutput(fmtJson(r), "success");
      showToast(`Watching ${p}`, "🔄");
      refreshWatch();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  document.querySelector<HTMLButtonElement>("#watchAddDir")!.onclick = async () => {
    const d = (document.querySelector<HTMLInputElement>("#watchDirPath")!).value.trim();
    if (!d) { showOutput("Enter a directory path", "error"); return; }
    try {
      const r = await client.watchAddDir(d);
      showOutput(fmtJson(r), "success");
      showToast(`Watching directory ${d}`, "🔄");
      refreshWatch();
    } catch (e) { showOutput(`Error: ${e}`, "error"); }
  };

  const connectBtn    = document.querySelector<HTMLButtonElement>("#watchConnect")!;
  const disconnectBtn = document.querySelector<HTMLButtonElement>("#watchDisconnect")!;
  const sseBadge      = document.querySelector<HTMLSpanElement>("#sseBadge")!;
  const eventLog      = document.querySelector<HTMLPreElement>("#watchEventLog")!;

  connectBtn.onclick = () => {
    if (watchEventSource) { watchEventSource.close(); watchEventSource = null; }
    watchEventSource = client.watchEvents();
    connectBtn.style.display    = "none";
    disconnectBtn.style.display = "";
    sseBadge.textContent = "● Connected";
    sseBadge.className   = "encore-tag success";
    sseBadge.style.display = "inline-flex";
    eventLog.textContent = "Connected — waiting for events…\n";

    watchEventSource.addEventListener("reload", (e: MessageEvent) => {
      try {
        const d = JSON.parse((e as MessageEvent).data);
        const ts = new Date(d.ts * 1000).toLocaleTimeString();
        eventLog.textContent += `[${ts}] ✓ reload: ${d.file}\n`;
        eventLog.scrollTop = eventLog.scrollHeight;
        showToast(`Hot reload: ${d.file}`, "🔄");
        refreshWatch();
      } catch { eventLog.textContent += `[reload] ${(e as MessageEvent).data}\n`; }
    });

    watchEventSource.addEventListener("error", (e: MessageEvent) => {
      try {
        const d = JSON.parse((e as MessageEvent).data);
        const ts = new Date(d.ts * 1000).toLocaleTimeString();
        eventLog.textContent += `[${ts}] ✗ error: ${d.file} — ${d.message}\n`;
        eventLog.scrollTop = eventLog.scrollHeight;
      } catch { eventLog.textContent += `[error] ${(e as MessageEvent).data}\n`; }
    });

    watchEventSource.onerror = () => {
      sseBadge.textContent = "● Disconnected";
      sseBadge.className   = "encore-tag error";
      connectBtn.style.display    = "";
      disconnectBtn.style.display = "none";
    };
  };

  disconnectBtn.onclick = () => {
    watchEventSource?.close();
    watchEventSource = null;
    connectBtn.style.display    = "";
    disconnectBtn.style.display = "none";
    sseBadge.textContent = "Disconnected";
    sseBadge.className   = "encore-tag";
  };

  refreshWatch();
}

function bindConfigEvents() {
  async function loadConfig() {
    try {
      const cfg = await client.config();
      const set = (id: string, v: string) => {
        const el = document.querySelector<HTMLElement>(`#${id}`);
        if (el) el.textContent = v;
      };
      set("cfgApp",     cfg.app);
      set("cfgVersion", cfg.version);
      set("cfgMode",    cfg.mode);
      set("cfgMw",      String(cfg.middleware.length));
      set("cfgRl",      String(cfg.ratelimit.length));
      set("cfgWatch",   cfg.watch.enabled ? `✓ (${cfg.watch.files.length} files)` : "off");
      const raw = document.querySelector<HTMLPreElement>("#cfgRaw");
      if (raw) raw.textContent = fmtJson(cfg);
      state.middlewareCount = cfg.middleware.length;
      state.rateLimitCount  = cfg.ratelimit.length;
    } catch (e) {
      const raw = document.querySelector<HTMLPreElement>("#cfgRaw");
      if (raw) raw.textContent = `Error loading config: ${e}`;
    }
  }

  document.querySelector<HTMLButtonElement>("#configRefresh")!.onclick = loadConfig;
  loadConfig();
}

// ── Initial Health Check ───────────────────────────────────────

async function checkInitialStatus() {
  try {
    await client.health();
    state.daemonOnline = true;
  } catch { state.daemonOnline = false; }

  try {
    const r = await client.redisStatus();
    state.redisOnline = r.addr !== "not running" && r.addr !== "";
  } catch { state.redisOnline = false; }

  try {
    const r = await client.dbStatus();
    state.dockerAvailable = !r.includes("not available");
  } catch { state.dockerAvailable = false; }

  try {
    const cfg = await client.config();
    state.middlewareCount = cfg.middleware.length;
    state.rateLimitCount  = cfg.ratelimit.length;
    state.watchFileCount  = cfg.watch.files.length;
  } catch { /* offline */ }

  mount();
}

// ── Bootstrap ──────────────────────────────────────────────────

mount();
checkInitialStatus();


// ── Event Binding Functions (New Views) ────────────────────────

function bindTracesEvents() {
  const refreshBtn = document.querySelector<HTMLButtonElement>("#tracesRefresh");
  const clearBtn = document.querySelector<HTMLButtonElement>("#tracesClear");
  
  if (refreshBtn) {
    refreshBtn.onclick = async () => {
      await updateTraces();
      showToast("Traces refreshed", "🔄");
    };
  }
  
  if (clearBtn) {
    clearBtn.onclick = async () => {
      try {
        await fetch(`${BASE_URL}/api/v1/traces`, { method: "DELETE" });
        showToast("Traces cleared", "✅");
        await updateTraces();
      } catch (e) {
        showOutput(`Error clearing traces: ${e}`, "error");
      }
    };
  }

  // Auto-refresh traces every 3 seconds
  const interval = setInterval(() => {
    if (state.activeView === "traces") updateTraces();
    else clearInterval(interval);
  }, 3000);

  // Initial load
  updateTraces();
}

function bindMetricsEvents() {
  const refreshBtn = document.querySelector<HTMLButtonElement>("#metricsRefresh");
  
  if (refreshBtn) {
    refreshBtn.onclick = async () => {
      await updateMetrics();
      showToast("Metrics refreshed", "🔄");
    };
  }

  // Auto-refresh metrics every 5 seconds
  const interval = setInterval(() => {
    if (state.activeView === "metrics") updateMetrics();
    else clearInterval(interval);
  }, 5000);

  // Initial load
  updateMetrics();
}

function bindServicesEvents() {
  const refreshBtn = document.querySelector<HTMLButtonElement>("#servicesRefresh");
  
  if (refreshBtn) {
    refreshBtn.onclick = async () => {
      await updateServices();
      showToast("Services refreshed", "🔄");
    };
  }

  // Initial load
  updateServices();
}