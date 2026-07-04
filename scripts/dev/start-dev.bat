@echo off
REM Bridge Framework - Windows Development Startup Script
REM This script starts the daemon and frontend together

echo.
echo ========================================
echo   Bridge Framework - Starting Dev
echo ========================================
echo.

REM Check if cargo is installed
where cargo >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Cargo not found. Please install Rust from https://rustup.rs/
    pause
    exit /b 1
)

REM Check if npm is installed
where npm >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] npm not found. Please install Node.js from https://nodejs.org/
    pause
    exit /b 1
)

echo [1/3] Building Bridge daemon...
cargo build -p daemon
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Failed to build daemon
    pause
    exit /b 1
)

echo.
echo [2/3] Installing frontend dependencies...
cd frontend
if not exist "node_modules" (
    echo Installing npm packages...
    call npm install
    if %ERRORLEVEL% NEQ 0 (
        echo [ERROR] npm install failed
        cd ..
        pause
        exit /b 1
    )
) else (
    echo Dependencies already installed. Run 'npm install' in frontend/ to update.
)
cd ..

echo.
echo [3/3] Starting servers...
echo.
echo   - Daemon will start on http://127.0.0.1:8787 (HTTP) and 127.0.0.1:7878 (TCP)
echo   - Frontend will start on http://localhost:5173
echo.
echo Press Ctrl+C to stop both servers
echo.

REM Start daemon in background
start "Bridge Daemon" cmd /c "cargo run -p daemon"

REM Wait a bit for daemon to start
timeout /t 2 /nobreak >nul

REM Start frontend (foreground)
cd frontend
call npm run dev

REM When frontend exits, we should also stop the daemon
echo.
echo Servers stopped.
pause
