#!/usr/bin/env bash
# Bridge Framework - Unix/Linux/Mac Development Startup Script
# This script starts the daemon and frontend together

set -e

echo ""
echo "========================================"
echo "  Bridge Framework - Starting Dev"
echo "========================================"
echo ""

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo "[ERROR] Cargo not found. Please install Rust from https://rustup.rs/"
    exit 1
fi

# Check if npm is installed
if ! command -v npm &> /dev/null; then
    echo "[ERROR] npm not found. Please install Node.js from https://nodejs.org/"
    exit 1
fi

# Build daemon
echo "[1/3] Building Bridge daemon..."
cargo build -p daemon

# Install frontend dependencies
echo ""
echo "[2/3] Installing frontend dependencies..."
cd frontend
if [ ! -d "node_modules" ]; then
    echo "Installing npm packages..."
    npm install
else
    echo "Dependencies already installed. Run 'npm install' in frontend/ to update."
fi
cd ..

# Start servers
echo ""
echo "[3/3] Starting servers..."
echo ""
echo "  - Daemon will start on http://127.0.0.1:8787 (HTTP) and 127.0.0.1:7878 (TCP)"
echo "  - Frontend will start on http://localhost:5173"
echo ""
echo "Press Ctrl+C to stop both servers"
echo ""

# Function to cleanup on exit
cleanup() {
    echo ""
    echo "Stopping servers..."
    kill $(jobs -p) 2>/dev/null
    exit
}

trap cleanup EXIT INT TERM

# Start daemon in background
cargo run -p daemon &
DAEMON_PID=$!

# Wait a bit for daemon to start
sleep 2

# Start frontend (foreground)
cd frontend
npm run dev

# Wait for frontend to exit
wait
