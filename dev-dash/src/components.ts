/**
 * Bridge Dev Dashboard - Enhanced Components
 * 
 * Provides:
 * - Real-time traces viewer
 * - Metrics dashboard
 * - Services inspector
 * - Live request streaming
 */

import { createDaemonClient } from "./daemon-client";

const BASE_URL = "http://127.0.0.1:8787";
const client = createDaemonClient(BASE_URL);

// ── Types ──────────────────────────────────────────────────────

interface Trace {
  id: string;
  service: string;
  endpoint: string;
  method: string;
  path: string;
  status: number;
  duration_ms: number;
  timestamp: number;
}

interface Service {
  name: string;
  auth: string;
  endpoints_count: number;
}

interface Metric {
  name: string;
  value: number;
  unit: string;
}

// ── Traces View ────────────────────────────────────────────────

export function renderTracesView(): string {
  return `
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Request Traces</div>
        <div class="encore-section-subtitle">Real-time request tracking and debugging</div>
      </div>
      <button id="tracesRefresh" class="encore-btn">🔄 Refresh</button>
      <button id="tracesClear" class="encore-btn">🗑️ Clear</button>
    </div>

    <div class="encore-grid encore-grid-3 encore-stagger">
      
      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">📊</div>Summary</div>
        </div>
        <div class="encore-stats">
          <div class="encore-stat">
            <div class="encore-stat-value" id="tracesTotal">0</div>
            <div class="encore-stat-label">Total Requests</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="tracesSuccess">0</div>
            <div class="encore-stat-label">Success (2xx)</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="tracesErrors">0</div>
            <div class="encore-stat-label">Errors (4xx/5xx)</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="tracesAvgLatency">—</div>
            <div class="encore-stat-label">Avg Latency</div>
          </div>
        </div>
      </div>

      <div class="encore-card encore-grid-2-col">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">📈</div>Status Distribution</div>
        </div>
        <div id="tracesStatus" style="padding: 16px; font-size: 13px; color: var(--encore-text-dim);">
          <div>Loading...</div>
        </div>
      </div>

    </div>

    <div class="encore-card encore-stagger">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon">⏱️</div>Recent Requests (Last 50)</div>
      </div>
      <div class="encore-table-wrapper" style="max-height: 400px; overflow-y: auto;">
        <table class="encore-table">
          <thead>
            <tr>
              <th>Time</th>
              <th>Method</th>
              <th>Path</th>
              <th>Service</th>
              <th>Status</th>
              <th>Latency</th>
            </tr>
          </thead>
          <tbody id="tracesList">
            <tr><td colspan="6" style="text-align: center; color: var(--encore-text-dim);">No traces yet</td></tr>
          </tbody>
        </table>
      </div>
    </div>

    <div class="encore-card encore-stagger">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon">📋</div>Trace Details</div>
      </div>
      <pre id="traceDetails" class="encore-output">Click a trace to see details.</pre>
    </div>
  `;
}

// ── Metrics View ───────────────────────────────────────────────

export function renderMetricsView(): string {
  return `
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Metrics</div>
        <div class="encore-section-subtitle">Performance and health metrics</div>
      </div>
      <button id="metricsRefresh" class="encore-btn">🔄 Refresh</button>
    </div>

    <div class="encore-grid encore-grid-4 encore-stagger">
      
      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">🚀</div>Throughput</div>
        </div>
        <div class="encore-stat" style="padding: 12px 0;">
          <div class="encore-stat-value" id="metricsThroughput">0</div>
          <div class="encore-stat-label">Requests/sec</div>
        </div>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">⏱️</div>Latency (p50)</div>
        </div>
        <div class="encore-stat" style="padding: 12px 0;">
          <div class="encore-stat-value" id="metricsLatencyP50">—</div>
          <div class="encore-stat-label">Median</div>
        </div>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">📍</div>Latency (p99)</div>
        </div>
        <div class="encore-stat" style="padding: 12px 0;">
          <div class="encore-stat-value" id="metricsLatencyP99">—</div>
          <div class="encore-stat-label">99th Percentile</div>
        </div>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">✅</div>Success Rate</div>
        </div>
        <div class="encore-stat" style="padding: 12px 0;">
          <div class="encore-stat-value" id="metricsSuccessRate">—</div>
          <div class="encore-stat-label">2xx Responses</div>
        </div>
      </div>

    </div>

    <div class="encore-grid encore-grid-2 encore-stagger">

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">📊</div>Requests by Status</div>
        </div>
        <pre id="metricsStatus" class="encore-output" style="max-height: 200px; overflow-y: auto;">Loading...</pre>
      </div>

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">🎯</div>Requests by Endpoint</div>
        </div>
        <pre id="metricsEndpoints" class="encore-output" style="max-height: 200px; overflow-y: auto;">Loading...</pre>
      </div>

    </div>

    <div class="encore-card encore-stagger">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon">📈</div>Prometheus Export</div>
      </div>
      <pre id="metricsPrometheus" class="encore-output" style="max-height: 300px; overflow-y: auto; font-size: 11px;">
# Prometheus metrics will appear here
# Pull from: GET /api/v1/metrics/prometheus
      </pre>
    </div>
  `;
}

// ── Services View ──────────────────────────────────────────────

export function renderServicesView(): string {
  return `
    <div class="encore-section-head">
      <div>
        <div class="encore-section-title">Services</div>
        <div class="encore-section-subtitle">Service catalog and endpoints</div>
      </div>
      <button id="servicesRefresh" class="encore-btn">🔄 Refresh</button>
    </div>

    <div class="encore-grid encore-grid-3 encore-stagger">

      <div class="encore-card">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">🎯</div>Overview</div>
        </div>
        <div class="encore-stats">
          <div class="encore-stat">
            <div class="encore-stat-value" id="servicesCount">0</div>
            <div class="encore-stat-label">Services</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="endpointsCount">0</div>
            <div class="encore-stat-label">Endpoints</div>
          </div>
          <div class="encore-stat">
            <div class="encore-stat-value" id="middlewareCount">0</div>
            <div class="encore-stat-label">Middleware</div>
          </div>
        </div>
      </div>

      <div class="encore-card encore-grid-2-col">
        <div class="encore-card-header">
          <div class="encore-card-title"><div class="encore-card-title-icon">🔐</div>Auth Schemes</div>
        </div>
        <div id="servicesAuth" style="padding: 12px; font-size: 13px; color: var(--encore-text-dim);">
          <div>Loading...</div>
        </div>
      </div>

    </div>

    <div class="encore-card encore-stagger">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon">📦</div>All Services</div>
      </div>
      <div id="servicesList" style="display: grid; gap: 12px;">
        <div style="padding: 24px; text-align: center; color: var(--encore-text-dim);">Loading services...</div>
      </div>
    </div>

    <div class="encore-card encore-stagger">
      <div class="encore-card-header">
        <div class="encore-card-title"><div class="encore-card-title-icon">🛣️</div>All Endpoints</div>
      </div>
      <div class="encore-table-wrapper" style="max-height: 500px; overflow-y: auto;">
        <table class="encore-table">
          <thead>
            <tr>
              <th>Service</th>
              <th>Method</th>
              <th>Path</th>
              <th>Auth</th>
              <th>Calls</th>
            </tr>
          </thead>
          <tbody id="endpointsList">
            <tr><td colspan="5" style="text-align: center; color: var(--encore-text-dim);">Loading endpoints...</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  `;
}

// ── Fetch Data Functions ───────────────────────────────────────

export async function fetchTraces(): Promise<Trace[]> {
  try {
    const response = await fetch(`${BASE_URL}/api/v1/traces?limit=50`);
    if (!response.ok) return [];
    const data = await response.json() as { traces: Trace[] };
    return data.traces || [];
  } catch {
    return [];
  }
}

export async function fetchMetrics() {
  try {
    const response = await fetch(`${BASE_URL}/api/v1/metrics`);
    if (!response.ok) return null;
    return await response.json();
  } catch {
    return null;
  }
}

export async function fetchServices(): Promise<Service[]> {
  try {
    const response = await fetch(`${BASE_URL}/api/v1/services`);
    if (!response.ok) return [];
    const data = await response.json() as { services: Service[] };
    return data.services || [];
  } catch {
    return [];
  }
}

export async function fetchRoutes() {
  try {
    const response = await fetch(`${BASE_URL}/api/v1/routes`);
    if (!response.ok) return [];
    const data = await response.json() as { routes: unknown[] };
    return data.routes || [];
  } catch {
    return [];
  }
}

// ── Update Functions ───────────────────────────────────────────

export async function updateTraces() {
  const traces = await fetchTraces();
  const tracesTotal = document.querySelector("#tracesTotal");
  const tracesSuccess = document.querySelector("#tracesSuccess");
  const tracesErrors = document.querySelector("#tracesErrors");
  const tracesAvgLatency = document.querySelector("#tracesAvgLatency");
  const tracesList = document.querySelector("#tracesList");

  if (!tracesTotal || !tracesList) return;

  const successful = traces.filter(t => t.status >= 200 && t.status < 300).length;
  const errors = traces.filter(t => t.status >= 400).length;
  const avgLatency = traces.length > 0 
    ? Math.round(traces.reduce((sum, t) => sum + t.duration_ms, 0) / traces.length)
    : 0;

  tracesTotal.textContent = String(traces.length);
  if (tracesSuccess) tracesSuccess.textContent = String(successful);
  if (tracesErrors) tracesErrors.textContent = String(errors);
  if (tracesAvgLatency) tracesAvgLatency.textContent = `${avgLatency}ms`;

  const rows = traces
    .slice(0, 50)
    .map(
      (t) => `
      <tr class="trace-row" data-id="${t.id}">
        <td style="font-size: 11px; color: var(--encore-text-dim);">${new Date(t.timestamp).toLocaleTimeString()}</td>
        <td><code class="encore-code-inline">${t.method}</code></td>
        <td><code class="encore-code-inline">${t.path}</code></td>
        <td><code class="encore-code-inline">${t.service}</code></td>
        <td><span class="encore-badge ${t.status < 300 ? 'success' : t.status < 400 ? 'info' : 'error'}">${t.status}</span></td>
        <td>${t.duration_ms}ms</td>
      </tr>
    `
    )
    .join("");

  tracesList.innerHTML = rows || '<tr><td colspan="6">No traces yet</td></tr>';
}

export async function updateMetrics() {
  const metrics = await fetchMetrics();
  if (!metrics) return;

  const metricsLatencyP50 = document.querySelector("#metricsLatencyP50");
  const metricsLatencyP99 = document.querySelector("#metricsLatencyP99");
  const metricsSuccessRate = document.querySelector("#metricsSuccessRate");

  if (metricsLatencyP50) metricsLatencyP50.textContent = `${metrics.latency_ms?.p50 ?? 0}ms`;
  if (metricsLatencyP99) metricsLatencyP99.textContent = `${metrics.latency_ms?.p99 ?? 0}ms`;
  if (metricsSuccessRate) {
    const total = Object.values(metrics.requests_by_status || {}).reduce((a: number, b: unknown) => a + (typeof b === 'number' ? b : 0), 0);
    const success = (metrics.requests_by_status?.['200'] || 0) as number;
    const rate = total > 0 ? Math.round((success / total) * 100) : 0;
    metricsSuccessRate.textContent = `${rate}%`;
  }
}

export async function updateServices() {
  const services = await fetchServices();
  const routes = await fetchRoutes();

  const servicesCount = document.querySelector("#servicesCount");
  const endpointsCount = document.querySelector("#endpointsCount");
  const servicesList = document.querySelector("#servicesList");
  const endpointsList = document.querySelector("#endpointsList");

  if (servicesCount) servicesCount.textContent = String(services.length);
  if (endpointsCount) endpointsCount.textContent = String(routes.length);

  if (servicesList) {
    const html = services.map(s => `
      <div class="encore-service-card" style="padding: 12px; border-radius: 6px; background: var(--encore-surface-2);">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
          <div style="font-weight: 600; color: white;">${s.name}</div>
          <span class="encore-badge">${s.endpoints_count} endpoints</span>
        </div>
        <div style="font-size: 12px; color: var(--encore-text-dim);">Auth: ${s.auth}</div>
      </div>
    `).join('');
    servicesList.innerHTML = html;
  }

  if (endpointsList) {
    const rows = (routes as any[]).slice(0, 100).map(r => `
      <tr>
        <td><code class="encore-code-inline">${r.service}</code></td>
        <td><code class="encore-code-inline">${r.method}</code></td>
        <td><code class="encore-code-inline">${r.path}</code></td>
        <td>${r.auth || 'none'}</td>
        <td>—</td>
      </tr>
    `).join('');
    endpointsList.innerHTML = rows || '<tr><td colspan="5">No endpoints</td></tr>';
  }
}
