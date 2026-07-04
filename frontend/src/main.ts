import "./style.css";
import { createDaemonClient } from "./daemon-client";
import { docPages, renderMarkdown } from "./docs";

// ── Configuration ──────────────────────────────────────────────

const BASE_URL = "http://127.0.0.1:8787";
const client = createDaemonClient(BASE_URL);

// ── State ──────────────────────────────────────────────────────

type View = "overview" | "api" | "database" | "docs";

interface AppState {
  activeView: View;
  activeDocId: string;
  daemonOnline: boolean;
  dockerAvailable: boolean;
  redisOnline: boolean;
  endpointCount: number;
}

const state: AppState = {
  activeView: "overview",
  activeDocId: docPages[0]?.id ?? "index",
  daemonOnline: false,
  dockerAvailable: false,
  redisOnline: false,
  endpointCount: 0,
};

// ── Root Mount Point ───────────────────────────────────────────

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Missing #app element");

// ── Header ─────────────────────────────────────────────────────

function renderHeader(): string {
  const tabs: { id: View; label: string; icon: string }[] = [
    { id: "overview", label: "Overview", icon: "⚡" },
    { id: "api", label: "API Explorer", icon: "🔌" },
    { id: "database", label: "Infrastructure", icon: "🗄️" },
    { id: "docs", label: "Docs", icon: "📖" },
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

// ── Shell Wrapper ──────────────────────────────────────────────

function shell(content: string): string {
  return `
    <div class="encore-shell">
      ${renderHeader()}
      <main class="encore-main encore-fade-in">${content}</main>
    </div>`;
}

// ── Overview View ──────────────────────────────────────────────

function renderOverview(): string {
  return shell(`
    <!-- Stats Row -->
    <div class="encore-stats" style="margin-bottom: 24px;">
      <div class="encore-stat">
        <div class="encore-stat-value" id="statEndpoints">${state.endpointCount}</div>
        <div class="encore-stat-label">Endpoints</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value"><span class="encore-status-dot ${state.daemonOnline ? "online" : "offline"}" style="display:inline-block;vertical-align:middle;"></span></div>
        <div class="encore-stat-label">Daemon</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value"><span class="encore-status-dot ${state.dockerAvailable ? "online" : "offline"}" style="display:inline-block;vertical-align:middle;"></span></div>
        <div class="encore-stat-label">Docker</div>
      </div>
      <div class="encore-stat">
        <div class="encore-stat-value"><span class="encore-status-dot ${state.redisOnline ? "online" : "offline"}" style="display:inline-block;vertical-align:middle;"></span></div>
        <div class="encore-stat-label">Redis</div>
      </div>
    </div>

    <!-- Architecture Flow -->
    <div class="encore-card" style="margin-bottom: 24px;">
      <div class="encore-card-header">
        <div>
          <div class="encore-card-title">
            <div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">⚡</div>
            Bridge Flow
          </div>
          <div class="encore-card-subtitle">Architecture overview — how your services connect</div>
        </div>
        <span class="encore-tag accent">Live</span>
      </div>
      <div class="encore-flow" id="flowDiagram">
        <div class="encore-flow-node compiler-node">
          <div class="node-icon">📝</div>
          <div class="node-title">.bridge DSL</div>
          <div class="node-subtitle">Source file</div>
        </div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node service-node">
          <div class="node-icon">⚙️</div>
          <div class="node-title">Compiler</div>
          <div class="node-subtitle">Parse + validate</div>
        </div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node service-node">
          <div class="node-icon">🔧</div>
          <div class="node-title">Codegen</div>
          <div class="node-subtitle">TypeScript client</div>
        </div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node db-node">
          <div class="node-icon">🐘</div>
          <div class="node-title">PostgreSQL</div>
          <div class="node-subtitle">Docker container</div>
        </div>
        <div class="encore-flow-arrow">→</div>
        <div class="encore-flow-node redis-node">
          <div class="node-icon">⚡</div>
          <div class="node-title">Miniredis</div>
          <div class="node-subtitle">Cache layer</div>
        </div>
      </div>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">
      <!-- Compiler Card -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title">
              <div class="encore-card-title-icon" style="background:var(--encore-success-dim);color:var(--encore-success);">⚙️</div>
              Compiler
            </div>
            <div class="encore-card-subtitle">Compile Bridge DSL to TypeScript clients</div>
          </div>
        </div>
        <textarea id="source" class="encore-textarea" rows="6" placeholder="service hello&#10;endpoint ping GET /ping&#10;endpoint echo POST /echo">service hello
endpoint ping GET /ping
endpoint echo POST /echo</textarea>
        <div class="encore-btn-group" style="margin-top: 12px;">
          <button id="compile" class="encore-btn encore-btn-primary">⚡ Compile + Codegen</button>
          <button id="latest" class="encore-btn">📦 Load Latest</button>
          <button id="parseEndpoints" class="encore-btn">🔍 Parse Endpoints</button>
        </div>
      </div>

      <!-- Daemon Controls Card -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title">
              <div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">🎛️</div>
              Daemon Controls
            </div>
            <div class="encore-card-subtitle">Backend at <code class="encore-code-inline">${BASE_URL}</code></div>
          </div>
          <span class="encore-tag ${state.daemonOnline ? "success" : "error"}">${state.daemonOnline ? "● Online" : "● Offline"}</span>
        </div>
        <div class="encore-btn-group" style="margin-bottom: 12px;">
          <button id="health" class="encore-btn encore-btn-success">❤️ Health</button>
          <button id="modeGet" class="encore-btn">📋 Get Mode</button>
        </div>
        <div style="display:flex;gap:8px;align-items:end;">
          <div style="flex:1;">
            <label class="encore-label">Daemon Mode</label>
            <input id="modeValue" value="full" class="encore-input" placeholder="lite | full | ultra | off" />
          </div>
          <button id="modeSet" class="encore-btn encore-btn-primary">Set Mode</button>
        </div>
      </div>

      <!-- Service Explorer (full width) -->
      <div class="encore-card encore-grid-full">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title">
              <div class="encore-card-title-icon" style="background:var(--encore-info-dim);color:var(--encore-info);">📡</div>
              Service Catalog
            </div>
            <div class="encore-card-subtitle">Parsed endpoints from your Bridge source</div>
          </div>
          <span class="encore-tag info" id="endpointCountBadge">0 endpoints</span>
        </div>
        <div id="endpointTable" style="overflow-x:auto;">
          <table class="encore-table">
            <thead>
              <tr>
                <th>Service</th>
                <th>Endpoint</th>
                <th>Method</th>
                <th>Path</th>
              </tr>
            </thead>
            <tbody id="endpointBody">
              <tr><td colspan="4" class="encore-empty-state">Click "Parse Endpoints" to see your service routes</td></tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <!-- Output Panel -->
    <div class="encore-card" style="margin-top: 20px;">
      <div class="encore-card-header">
        <div class="encore-card-title">
          <div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>
          Output
        </div>
      </div>
      <pre id="output" class="encore-output">Ready. Click a button above to get started.</pre>
    </div>
  `);
}

// ── API Explorer View ──────────────────────────────────────────

function renderApiExplorer(): string {
  return shell(`
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">API Explorer</div>
        <div class="encore-section-subtitle">Test your daemon HTTP endpoints interactively</div>
      </div>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">
      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title">
            <div class="encore-card-title-icon" style="background:var(--encore-accent-glow);color:var(--encore-accent-hover);">🔌</div>
            Request
          </div>
        </div>
        <div style="margin-bottom: 12px;">
          <label class="encore-label">Endpoint</label>
          <select id="apiEndpoint" class="encore-select" style="width:100%;">
            <option value="GET /health">GET /health</option>
            <option value="GET /mode">GET /mode</option>
            <option value="POST /mode">POST /mode</option>
            <option value="POST /compile">POST /compile</option>
            <option value="GET /db/latest">GET /db/latest</option>
            <option value="POST /db/create">POST /db/create</option>
            <option value="GET /db/status">GET /db/status</option>
            <option value="POST /db/migrate">POST /db/migrate</option>
            <option value="DELETE /db/destroy">DELETE /db/destroy</option>
            <option value="GET /redis/status">GET /redis/status</option>
          </select>
        </div>
        <div style="margin-bottom: 12px;">
          <label class="encore-label">Request Body</label>
          <textarea id="apiBody" class="encore-textarea" rows="4" placeholder="Request body (for POST/DELETE)"></textarea>
        </div>
        <button id="apiSend" class="encore-btn encore-btn-primary" style="width:100%;">▶ Send Request</button>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title">
            <div class="encore-card-title-icon" style="background:var(--encore-success-dim);color:var(--encore-success);">📨</div>
            Response
          </div>
          <span class="encore-tag" id="apiStatusTag" style="display:none;"></span>
        </div>
        <pre id="apiResponse" class="encore-output" style="min-height:200px;">Response will appear here.</pre>
      </div>
    </div>

    <!-- Endpoint Reference Table -->
    <div class="encore-card" style="margin-top:20px;">
      <div class="encore-card-header">
        <div class="encore-card-title">
          <div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>
          Endpoint Reference
        </div>
      </div>
      <table class="encore-table">
        <thead>
          <tr><th>Method</th><th>Path</th><th>Description</th></tr>
        </thead>
        <tbody>
          <tr><td><span class="encore-method-tag get">GET</span></td><td><code class="encore-code-inline">/health</code></td><td>Daemon health check</td></tr>
          <tr><td><span class="encore-method-tag get">GET</span></td><td><code class="encore-code-inline">/mode</code></td><td>Get current daemon mode</td></tr>
          <tr><td><span class="encore-method-tag post">POST</span></td><td><code class="encore-code-inline">/mode</code></td><td>Set daemon mode (body: lite|full|ultra|off)</td></tr>
          <tr><td><span class="encore-method-tag post">POST</span></td><td><code class="encore-code-inline">/compile</code></td><td>Compile Bridge DSL source</td></tr>
          <tr><td><span class="encore-method-tag get">GET</span></td><td><code class="encore-code-inline">/db/latest</code></td><td>Get latest codegen output</td></tr>
          <tr><td><span class="encore-method-tag post">POST</span></td><td><code class="encore-code-inline">/db/create</code></td><td>Create Docker Postgres container</td></tr>
          <tr><td><span class="encore-method-tag get">GET</span></td><td><code class="encore-code-inline">/db/status</code></td><td>Check container status</td></tr>
          <tr><td><span class="encore-method-tag post">POST</span></td><td><code class="encore-code-inline">/db/migrate</code></td><td>Run SQL migration</td></tr>
          <tr><td><span class="encore-method-tag delete">DELETE</span></td><td><code class="encore-code-inline">/db/destroy</code></td><td>Stop and remove container</td></tr>
          <tr><td><span class="encore-method-tag get">GET</span></td><td><code class="encore-code-inline">/redis/status</code></td><td>Miniredis server status</td></tr>
        </tbody>
      </table>
    </div>
  `);
}

// ── Infrastructure View (Database + Redis) ─────────────────────

function renderInfrastructure(): string {
  return shell(`
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Infrastructure</div>
        <div class="encore-section-subtitle">Manage databases, caching, and Docker services</div>
      </div>
    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">
      <!-- Database Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title">
              <div class="encore-card-title-icon" style="background:var(--encore-postgres-dim);color:var(--encore-postgres);">🐘</div>
              PostgreSQL
            </div>
            <div class="encore-card-subtitle">Docker container management</div>
          </div>
          <span class="encore-tag ${state.dockerAvailable ? "success" : "warning"}">${state.dockerAvailable ? "Docker ✓" : "Docker ?"}</span>
        </div>

        <div style="margin-bottom: 12px;">
          <label class="encore-label">Container Name</label>
          <input id="dbName" value="default" class="encore-input" placeholder="Container name (e.g. myapp)" />
        </div>

        <div class="encore-btn-group" style="margin-bottom: 16px;">
          <button id="dbCreate" class="encore-btn encore-btn-success">＋ Create DB</button>
          <button id="dbStatus" class="encore-btn">📊 Status</button>
          <button id="dbDestroy" class="encore-btn encore-btn-danger">✕ Destroy</button>
        </div>

        <hr class="encore-divider" />

        <div style="margin-bottom: 12px;">
          <label class="encore-label">SQL Migration</label>
          <textarea id="migrateSql" class="encore-textarea" rows="4" placeholder="CREATE TABLE users (&#10;  id SERIAL PRIMARY KEY,&#10;  name TEXT NOT NULL,&#10;  created_at TIMESTAMPTZ DEFAULT NOW()&#10;);"></textarea>
        </div>
        <button id="dbMigrate" class="encore-btn encore-btn-primary" style="width:100%;">▶ Run Migration</button>
      </div>

      <!-- Redis Panel -->
      <div class="encore-card">
        <div class="encore-card-header">
          <div>
            <div class="encore-card-title">
              <div class="encore-card-title-icon" style="background:var(--encore-redis-dim);color:var(--encore-redis);">⚡</div>
              Miniredis
            </div>
            <div class="encore-card-subtitle">Embedded Redis-compatible cache server</div>
          </div>
          <span class="encore-tag ${state.redisOnline ? "success" : "error"}">${state.redisOnline ? "● Running" : "● Stopped"}</span>
        </div>

        <div class="encore-stats" style="margin-bottom: 16px;">
          <div class="encore-stat">
            <div class="encore-stat-value" id="redisAddr">—</div>
            <div class="encore-stat-label">Address</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="redisConns">—</div>
            <div class="encore-stat-label">Connections</div>
          </div>
        </div>

        <button id="redisStatus" class="encore-btn" style="width:100%;margin-bottom:16px;">🔄 Refresh Status</button>

        <hr class="encore-divider" />

        <div>
          <div class="encore-card-title" style="margin-bottom:8px;font-size:13px;">
            <div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);width:24px;height:24px;font-size:12px;">📋</div>
            Supported Commands
          </div>
          <div style="display:flex;flex-wrap:wrap;gap:6px;">
            ${["PING", "SET", "GET", "DEL", "EXISTS", "KEYS", "EXPIRE", "TTL", "COMMAND"]
              .map((c) => `<span class="encore-tag accent">${c}</span>`)
              .join("")}
          </div>
        </div>

        <pre id="redisOutput" class="encore-output" style="margin-top:16px;min-height:60px;">Click "Refresh Status" to see miniredis state.</pre>
      </div>
    </div>

    <!-- Infrastructure Output -->
    <div class="encore-card" style="margin-top: 20px;">
      <div class="encore-card-header">
        <div class="encore-card-title">
          <div class="encore-card-title-icon" style="background:var(--encore-surface-2);color:var(--encore-text-dim);">📋</div>
          Infrastructure Output
        </div>
      </div>
      <pre id="output" class="encore-output">Infrastructure output will appear here.</pre>
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

// ── Source Endpoint Parser ─────────────────────────────────────

interface ParsedEndpoint {
  service: string;
  name: string;
  method: string;
  path: string;
}

function parseSourceEndpoints(source: string): ParsedEndpoint[] {
  const lines = source
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  let currentService = "unknown";
  const endpoints: ParsedEndpoint[] = [];
  for (const line of lines) {
    if (line.startsWith("service ")) {
      currentService = line.slice(8).trim();
    } else if (line.startsWith("endpoint ")) {
      const parts = line.slice(9).trim().split(/\s+/);
      if (parts.length >= 3) {
        endpoints.push({
          service: currentService,
          name: parts[0],
          method: parts[1],
          path: parts[2],
        });
      }
    }
  }
  return endpoints;
}

// ── Show helper ────────────────────────────────────────────────

function showOutput(value: string, type?: "success" | "error" | "accent") {
  const output = document.querySelector<HTMLPreElement>("#output");
  if (output) {
    output.textContent = value;
    output.className = "encore-output" + (type ? ` ${type}` : "");
  }
}

// ── Toast helper ───────────────────────────────────────────────

function showToast(message: string, icon = "✓") {
  const existing = document.querySelector(".encore-toast");
  if (existing) existing.remove();
  const toast = document.createElement("div");
  toast.className = "encore-toast";
  toast.innerHTML = `<span>${icon}</span> ${message}`;
  document.body.appendChild(toast);
  setTimeout(() => toast.remove(), 3000);
}

// ── Mount + Bind Events ────────────────────────────────────────

function mount() {
  switch (state.activeView) {
    case "overview":
      app.innerHTML = renderOverview();
      break;
    case "api":
      app.innerHTML = renderApiExplorer();
      break;
    case "database":
      app.innerHTML = renderInfrastructure();
      break;
    case "docs":
      app.innerHTML = renderDocs();
      break;
  }

  bindNavigation();

  if (state.activeView === "overview") bindOverviewEvents();
  if (state.activeView === "api") bindApiExplorerEvents();
  if (state.activeView === "database") bindInfrastructureEvents();
}

// ── Navigation Binding ─────────────────────────────────────────

function bindNavigation() {
  app.querySelectorAll<HTMLButtonElement>(".encore-nav-tab").forEach((btn) => {
    btn.onclick = () => {
      state.activeView = (btn.dataset.view as View) ?? "overview";
      mount();
    };
  });

  app.querySelectorAll<HTMLButtonElement>(".encore-sidebar-item").forEach((btn) => {
    btn.onclick = () => {
      state.activeDocId = btn.dataset.doc ?? "index";
      mount();
    };
  });
}

// ── Overview Events ────────────────────────────────────────────

function bindOverviewEvents() {
  const source = document.querySelector<HTMLTextAreaElement>("#source");
  const modeValue = document.querySelector<HTMLInputElement>("#modeValue");
  if (!source || !modeValue) return;

  document.querySelector<HTMLButtonElement>("#health")!.onclick = async () => {
    try {
      const result = await client.health();
      showOutput(result, "success");
      showToast("Daemon is healthy", "❤️");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#modeGet")!.onclick = async () => {
    try {
      showOutput(await client.modeGet());
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#modeSet")!.onclick = async () => {
    try {
      const result = await client.modeSet(modeValue.value);
      showOutput(result, "success");
      showToast(`Mode set to ${modeValue.value}`);
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#compile")!.onclick = async () => {
    try {
      const result = await client.compile(source.value);
      showOutput(result, "accent");
      showToast("Compiled successfully", "⚡");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#latest")!.onclick = async () => {
    try {
      showOutput(await client.latest(), "accent");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#parseEndpoints")!.onclick = () => {
    const endpoints = parseSourceEndpoints(source.value);
    state.endpointCount = endpoints.length;
    const body = document.querySelector<HTMLTableSectionElement>("#endpointBody")!;
    const badge = document.querySelector<HTMLSpanElement>("#endpointCountBadge");
    const statEl = document.querySelector<HTMLDivElement>("#statEndpoints");

    if (badge) badge.textContent = `${endpoints.length} endpoint${endpoints.length !== 1 ? "s" : ""}`;
    if (statEl) statEl.textContent = String(endpoints.length);

    if (endpoints.length === 0) {
      body.innerHTML = `<tr><td colspan="4" class="encore-empty-state"><div class="encore-empty-state-icon">📡</div>No endpoints found. Write Bridge DSL above and click Parse.</td></tr>`;
    } else {
      body.innerHTML = endpoints
        .map(
          (ep) => `<tr>
            <td><span class="encore-tag accent">${ep.service}</span></td>
            <td style="font-weight:600;color:white;">${ep.name}</td>
            <td><span class="encore-method-tag ${ep.method.toLowerCase()}">${ep.method}</span></td>
            <td><code class="encore-code-inline">${ep.path}</code></td>
          </tr>`
        )
        .join("");
      showToast(`Parsed ${endpoints.length} endpoints`, "📡");
    }
  };
}

// ── API Explorer Events ────────────────────────────────────────

function bindApiExplorerEvents() {
  const apiEndpoint = document.querySelector<HTMLSelectElement>("#apiEndpoint")!;
  const apiBody = document.querySelector<HTMLTextAreaElement>("#apiBody")!;
  const apiResponse = document.querySelector<HTMLPreElement>("#apiResponse")!;
  const apiStatusTag = document.querySelector<HTMLSpanElement>("#apiStatusTag")!;

  document.querySelector<HTMLButtonElement>("#apiSend")!.onclick = async () => {
    const [method, ...pathParts] = apiEndpoint.value.split(" ");
    const path = pathParts.join(" ");
    apiResponse.textContent = "Sending...";
    apiResponse.className = "encore-output";
    apiStatusTag.style.display = "none";

    try {
      const opts: RequestInit = { method };
      if (method === "POST" || method === "DELETE") {
        opts.body = apiBody.value;
      }
      const resp = await fetch(`${BASE_URL}${path}`, opts);
      const text = await resp.text();

      // Try to pretty-print JSON
      let formatted = text;
      try {
        formatted = JSON.stringify(JSON.parse(text), null, 2);
      } catch { /* not JSON */ }

      apiResponse.textContent = `HTTP ${resp.status} ${resp.statusText}\n\n${formatted}`;
      apiResponse.className = `encore-output ${resp.ok ? "success" : "error"}`;

      apiStatusTag.textContent = `${resp.status} ${resp.statusText}`;
      apiStatusTag.className = `encore-tag ${resp.ok ? "success" : "error"}`;
      apiStatusTag.style.display = "inline-flex";
    } catch (err) {
      apiResponse.textContent = `Network Error: ${err}`;
      apiResponse.className = "encore-output error";
      apiStatusTag.textContent = "Error";
      apiStatusTag.className = "encore-tag error";
      apiStatusTag.style.display = "inline-flex";
    }
  };
}

// ── Infrastructure Events ──────────────────────────────────────

function bindInfrastructureEvents() {
  const dbName = document.querySelector<HTMLInputElement>("#dbName")!;
  const migrateSql = document.querySelector<HTMLTextAreaElement>("#migrateSql")!;
  const redisOutput = document.querySelector<HTMLPreElement>("#redisOutput")!;

  document.querySelector<HTMLButtonElement>("#dbCreate")!.onclick = async () => {
    try {
      const result = await client.dbCreate(dbName.value || "default");
      showOutput(result, "success");
      showToast("Database created", "🐘");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#dbStatus")!.onclick = async () => {
    try {
      const result = await client.dbStatus();
      showOutput(result);
      state.dockerAvailable = !result.includes("not available");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#dbDestroy")!.onclick = async () => {
    try {
      const result = await client.dbDestroy(dbName.value || "default");
      showOutput(result, "error");
      showToast("Database destroyed", "✕");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#dbMigrate")!.onclick = async () => {
    try {
      const result = await client.dbMigrate(migrateSql.value);
      showOutput(result, "success");
      showToast("Migration executed", "▶");
    } catch (e) {
      showOutput(`Error: ${e}`, "error");
    }
  };

  document.querySelector<HTMLButtonElement>("#redisStatus")!.onclick = async () => {
    try {
      const result = await client.redisStatus();
      redisOutput.textContent = result;
      showOutput(result);

      // Try to parse Redis status for display
      try {
        const json = JSON.parse(result);
        const addrEl = document.querySelector<HTMLDivElement>("#redisAddr");
        const connsEl = document.querySelector<HTMLDivElement>("#redisConns");
        if (addrEl) addrEl.textContent = json.addr ?? "—";
        if (connsEl) connsEl.textContent = String(json.connections ?? "—");
        state.redisOnline = json.addr !== "not running";
      } catch {
        // Non-JSON response, try key=value format
        const addrMatch = result.match(/addr=(\S+)/);
        const connMatch = result.match(/connections=(\d+)/);
        const addrEl = document.querySelector<HTMLDivElement>("#redisAddr");
        const connsEl = document.querySelector<HTMLDivElement>("#redisConns");
        if (addrEl && addrMatch) addrEl.textContent = addrMatch[1];
        if (connsEl && connMatch) connsEl.textContent = connMatch[1];
        state.redisOnline = !result.includes("not running");
      }
      showToast("Redis status refreshed", "⚡");
    } catch (e) {
      redisOutput.textContent = `Error: ${e}`;
      showOutput(`Error: ${e}`, "error");
    }
  };
}

// ── Initial Health Check ───────────────────────────────────────

async function checkInitialStatus() {
  try {
    await client.health();
    state.daemonOnline = true;
  } catch {
    state.daemonOnline = false;
  }

  try {
    const result = await client.redisStatus();
    state.redisOnline = !result.includes("not running");
  } catch {
    state.redisOnline = false;
  }

  try {
    const result = await client.dbStatus();
    state.dockerAvailable = !result.includes("not available");
  } catch {
    state.dockerAvailable = false;
  }

  mount(); // Re-render with updated status
}

// ── Bootstrap ──────────────────────────────────────────────────

mount();
checkInitialStatus();
