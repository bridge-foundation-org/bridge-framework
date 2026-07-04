# Bridge Benefits

How Bridge helps you ship typed APIs faster.

Using Bridge to declare services in a simple DSL unlocks several benefits:

- **Local development with instant codegen**: Compile `.bridge` files and get TypeScript clients immediately.
- **Rapid feedback**: Catch invalid endpoints before wiring the frontend.
- **No manual client maintenance**: Generated clients stay aligned with your service definition.
- **Unified toolchain**: One daemon exposes compile, mode, and storage over HTTP and TCP.
- **Frontend-ready defaults**: Vite + Tailwind dev UI ships with the framework repo.

## Ponytail modes

Bridge supports lazy-dev modes via `bridge mode-set`: `lite`, `full`, `ultra`, and `off`.
