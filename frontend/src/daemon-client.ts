const baseUrl = "http://127.0.0.1:8787";

export function createDaemonClient(url = baseUrl) {
  return {
    async health() {
      const response = await fetch(`${url}/health`);
      if (!response.ok) throw new Error(`request failed: ${response.status}`);
      return response.text();
    },
    async modeGet() {
      const response = await fetch(`${url}/mode`);
      if (!response.ok) throw new Error(`request failed: ${response.status}`);
      return response.text();
    },
    async modeSet(mode: string) {
      const response = await fetch(`${url}/mode`, { method: "POST", body: mode });
      if (!response.ok) throw new Error(`request failed: ${response.status}`);
      return response.text();
    },
    async compile(source: string) {
      const response = await fetch(`${url}/compile`, { method: "POST", body: source });
      if (!response.ok) throw new Error(await response.text());
      return response.text();
    },
    async latest() {
      const response = await fetch(`${url}/db/latest`);
      if (!response.ok) throw new Error(await response.text());
      return response.text();
    },
    // ── Database Docker management ──
    async dbCreate(name: string) {
      const response = await fetch(`${url}/db/create`, { method: "POST", body: name });
      return response.text();
    },
    async dbStatus() {
      const response = await fetch(`${url}/db/status`);
      return response.text();
    },
    async dbMigrate(sql: string) {
      const response = await fetch(`${url}/db/migrate`, { method: "POST", body: sql });
      return response.text();
    },
    async dbDestroy(name: string) {
      const response = await fetch(`${url}/db/destroy`, { method: "DELETE", body: name });
      return response.text();
    },
    // ── Redis ──
    async redisStatus() {
      const response = await fetch(`${url}/redis/status`);
      return response.text();
    },
  };
}
