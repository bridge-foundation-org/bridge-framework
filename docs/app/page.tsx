export default function Home() {
  return (
    <div className="min-h-screen bg-gradient-to-br from-bridge-50 to-white">
      {/* Hero Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8">
        <div className="max-w-4xl mx-auto text-center">
          <h1 className="text-5xl sm:text-6xl font-bold text-gray-900 mb-6">
            Build Type-Safe Backend Services in <span className="text-bridge-600">Rust</span>
          </h1>
          <p className="text-xl text-gray-600 mb-8">
            Bridge Framework is a lightweight, Encore-inspired framework for building production-grade backend services with zero external dependencies.
          </p>
          <div className="flex gap-4 justify-center">
            <a href="/getting-started" className="button-primary text-lg px-8 py-3">
              Get Started →
            </a>
            <a href="https://github.com/yourusername/bridge-framework" className="button-secondary text-lg px-8 py-3">
              View on GitHub
            </a>
          </div>
        </div>
      </section>

      {/* Features Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8 bg-white">
        <div className="max-w-6xl mx-auto">
          <h2 className="text-4xl font-bold text-center text-gray-900 mb-16">
            Features Built for Developers
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <div className="card">
              <div className="text-3xl mb-4">🎯</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Zero Config</h3>
              <p className="text-gray-600">Start building services in seconds with sensible defaults and minimal setup.</p>
            </div>
            <div className="card">
              <div className="text-3xl mb-4">🔒</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Type Safe</h3>
              <p className="text-gray-600">Automatically generate type-safe TypeScript and Go clients that match your backend.</p>
            </div>
            <div className="card">
              <div className="text-3xl mb-4">⚡</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Fast Development</h3>
              <p className="text-gray-600">Hot reload, built-in dev dashboard, and instant feedback on changes.</p>
            </div>
            <div className="card">
              <div className="text-3xl mb-4">🐘</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Docker Ready</h3>
              <p className="text-gray-600">PostgreSQL and Redis containers managed automatically for your services.</p>
            </div>
            <div className="card">
              <div className="text-3xl mb-4">📊</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Observable</h3>
              <p className="text-gray-600">Built-in tracing, metrics, and logging with beautiful dev dashboard.</p>
            </div>
            <div className="card">
              <div className="text-3xl mb-4">🚀</div>
              <h3 className="text-xl font-semibold text-gray-900 mb-2">Production Ready</h3>
              <p className="text-gray-600">Deploy anywhere with built-in authentication, rate limiting, and graceful shutdown.</p>
            </div>
          </div>
        </div>
      </section>

      {/* Quick Start Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8">
        <div className="max-w-4xl mx-auto">
          <h2 className="text-4xl font-bold text-gray-900 mb-8">Quick Start</h2>
          <div className="bg-gray-900 text-gray-100 rounded-lg p-6 overflow-x-auto">
            <pre className="text-sm">
{`# Clone the framework
git clone https://github.com/yourusername/bridge-framework
cd bridge-framework

# Build everything
cargo build --workspace

# Start the dev dashboard
cd dev-dash && npm install && npm run dev

# In another terminal, start the daemon
cargo run -p daemon

# Your dashboard is ready at http://localhost:5173 🎉`}
            </pre>
          </div>
          <p className="text-gray-600 mt-4">
            See the <a href="/getting-started" className="text-bridge-600 hover:text-bridge-700 font-semibold">Getting Started guide</a> for detailed instructions.
          </p>
        </div>
      </section>

      {/* Architecture Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8 bg-gray-50">
        <div className="max-w-4xl mx-auto">
          <h2 className="text-4xl font-bold text-gray-900 mb-8">Inspired by Encore</h2>
          <p className="text-lg text-gray-600 mb-6">
            Bridge Framework takes the brilliant ideas from <a href="https://encore.dev" className="text-bridge-600 hover:text-bridge-700 font-semibold">Encore</a> and reimplements them in Rust with different tradeoffs:
          </p>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
            <div>
              <h3 className="text-xl font-semibold text-gray-900 mb-4">Bridge</h3>
              <ul className="space-y-2 text-gray-600">
                <li>✅ Pure Rust (stdlib only)</li>
                <li>✅ Self-hosted deployment</li>
                <li>✅ Full control & transparency</li>
                <li>✅ Learning resource</li>
                <li>✅ Free & open source</li>
              </ul>
            </div>
            <div>
              <h3 className="text-xl font-semibold text-gray-900 mb-4">Encore</h3>
              <ul className="space-y-2 text-gray-600">
                <li>✅ Go + TypeScript</li>
                <li>✅ Cloud-managed platform</li>
                <li>✅ Turnkey operations</li>
                <li>✅ Enterprise support</li>
                <li>✅ Paid tiers</li>
              </ul>
            </div>
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8">
        <div className="max-w-4xl mx-auto text-center">
          <h2 className="text-4xl font-bold text-gray-900 mb-6">Ready to Build?</h2>
          <p className="text-xl text-gray-600 mb-8">
            Start building your first service with Bridge Framework today.
          </p>
          <div className="flex gap-4 justify-center">
            <a href="/getting-started" className="button-primary text-lg px-8 py-3">
              Get Started
            </a>
            <a href="/examples" className="button-secondary text-lg px-8 py-3">
              See Examples
            </a>
          </div>
        </div>
      </section>
    </div>
  )
}
