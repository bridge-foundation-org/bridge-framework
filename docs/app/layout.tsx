import type { Metadata } from 'next'
import './styles/globals.css'

export const metadata: Metadata = {
  title: 'Bridge Framework - Developer Documentation',
  description: 'Build type-safe backend services in Rust with Bridge Framework, inspired by Encore.',
  keywords: ['Rust', 'Backend', 'Framework', 'Type-safe', 'Microservices'],
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en">
      <body className="antialiased">
        <div className="min-h-screen flex flex-col">
          <header className="border-b border-gray-200 bg-white sticky top-0 z-50">
            <nav className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 flex items-center justify-between">
              <div className="flex items-center gap-8">
                <a href="/" className="text-2xl font-bold text-bridge-600">
                  Bridge
                </a>
                <div className="hidden md:flex gap-1">
                  <a href="/getting-started" className="nav-link">Docs</a>
                  <a href="/api-reference" className="nav-link">API</a>
                  <a href="/examples" className="nav-link">Examples</a>
                  <a href="/contributing" className="nav-link">Contributing</a>
                </div>
              </div>
              <div className="flex items-center gap-4">
                <a
                  href="https://github.com/yourusername/bridge-framework"
                  className="text-sm text-gray-600 hover:text-gray-900"
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  GitHub
                </a>
              </div>
            </nav>
          </header>

          <main className="flex-1">
            {children}
          </main>

          <footer className="border-t border-gray-200 bg-gray-50">
            <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12">
              <div className="grid grid-cols-1 md:grid-cols-4 gap-8 mb-8">
                <div>
                  <h3 className="font-semibold text-gray-900 mb-4">Product</h3>
                  <ul className="space-y-2 text-sm text-gray-600">
                    <li><a href="/getting-started" className="hover:text-bridge-600">Getting Started</a></li>
                    <li><a href="/api-reference" className="hover:text-bridge-600">API Reference</a></li>
                    <li><a href="/examples" className="hover:text-bridge-600">Examples</a></li>
                  </ul>
                </div>
                <div>
                  <h3 className="font-semibold text-gray-900 mb-4">Community</h3>
                  <ul className="space-y-2 text-sm text-gray-600">
                    <li><a href="/contributing" className="hover:text-bridge-600">Contributing</a></li>
                    <li><a href="https://github.com/yourusername/bridge-framework/discussions" className="hover:text-bridge-600">Discussions</a></li>
                    <li><a href="https://github.com/yourusername/bridge-framework/issues" className="hover:text-bridge-600">Issues</a></li>
                  </ul>
                </div>
                <div>
                  <h3 className="font-semibold text-gray-900 mb-4">Resources</h3>
                  <ul className="space-y-2 text-sm text-gray-600">
                    <li><a href="/troubleshooting" className="hover:text-bridge-600">Troubleshooting</a></li>
                    <li><a href="https://github.com/yourusername/bridge-framework" className="hover:text-bridge-600">Source Code</a></li>
                    <li><a href="https://github.com/yourusername/bridge-framework/blob/main/LICENSE" className="hover:text-bridge-600">License</a></li>
                  </ul>
                </div>
                <div>
                  <h3 className="font-semibold text-gray-900 mb-4">Inspiration</h3>
                  <ul className="space-y-2 text-sm text-gray-600">
                    <li><a href="https://encore.dev" className="hover:text-bridge-600">Encore Framework</a></li>
                    <li><a href="https://www.rust-lang.org/" className="hover:text-bridge-600">Rust</a></li>
                  </ul>
                </div>
              </div>
              <div className="border-t border-gray-200 pt-8 text-center text-sm text-gray-600">
                <p>&copy; 2026 Bridge Framework. All rights reserved.</p>
              </div>
            </div>
          </footer>
        </div>
      </body>
    </html>
  )
}
