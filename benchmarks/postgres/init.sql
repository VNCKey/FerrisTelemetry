CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO users (username, email, role) VALUES
    ('ferris_crab', 'ferris@rust-lang.org', 'admin'),
    ('tokio_runner', 'tokio@async.rs', 'core'),
    ('axum_master', 'axum@tower.rs', 'developer'),
    ('ratatui_artist', 'ratatui@tui.rs', 'designer'),
    ('crossterm_io', 'terminal@crossterm.rs', 'engineer'),
    ('sqlx_query', 'sqlx@database.rs', 'dba'),
    ('sysinfo_agent', 'monitor@kernel.org', 'devops'),
    ('fiber_rival', 'fiber@gofiber.io', 'guest'),
    ('fastapi_star', 'fastapi@tiangolo.com', 'guest'),
    ('postgres_power', 'postgres@benchmark.local', 'database')
ON CONFLICT (username) DO NOTHING;
