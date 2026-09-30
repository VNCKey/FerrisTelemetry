-- Deterministic dataset for the professional PostgreSQL benchmark.
-- It is safe to run repeatedly against an existing database.
INSERT INTO users (id, username, email, role)
SELECT id,
       'benchmark_user_' || id,
       'benchmark_user_' || id || '@benchmark.local',
       CASE WHEN id % 10 = 0 THEN 'operator' ELSE 'user' END
FROM generate_series(11, 100000) AS id
ON CONFLICT (id) DO NOTHING;

SELECT setval(
    pg_get_serial_sequence('users', 'id'),
    GREATEST((SELECT MAX(id) FROM users), 1),
    true
);

CREATE TABLE IF NOT EXISTS benchmark_writes (
    id BIGINT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ANALYZE users;
ANALYZE benchmark_writes;
