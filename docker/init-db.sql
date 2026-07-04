-- Bridge Framework — Initial Database Setup
-- This script runs automatically when the Postgres container starts

-- Create sample schema for testing
CREATE SCHEMA IF NOT EXISTS bridge_app;

-- Example table for tutorials
CREATE TABLE IF NOT EXISTS bridge_app.users (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Example index
CREATE INDEX IF NOT EXISTS idx_users_email ON bridge_app.users(email);

-- Insert sample data
INSERT INTO bridge_app.users (name, email) VALUES
    ('Alice', 'alice@example.com'),
    ('Bob', 'bob@example.com')
ON CONFLICT (email) DO NOTHING;

-- Grant permissions
GRANT ALL PRIVILEGES ON SCHEMA bridge_app TO bridge;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA bridge_app TO bridge;
GRANT ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA bridge_app TO bridge;

-- Success message
DO $$
BEGIN
    RAISE NOTICE 'Bridge database initialized successfully!';
END $$;
