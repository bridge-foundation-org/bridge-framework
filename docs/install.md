# Installation

Install the Bridge CLI to get started with local development.

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- Git Bash or WSL (for shell scripts)

## Install the Bridge CLI

```bash
cargo install --path cli
bridge help
```

## Start the daemon

```bash
cargo run -p daemon
```

The HTTP API listens on `127.0.0.1:8787`.

## Create a new app

```bash
bridge init my-app
cd my-app/frontend
npm install
npm run generate-client:local
npm run dev
```

## Verify your setup

```bash
./check.bash
```
