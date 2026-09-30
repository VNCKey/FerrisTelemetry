import os
from contextlib import asynccontextmanager

import asyncpg
from fastapi import FastAPI, HTTPException


DATABASE_URL = os.getenv(
    "DATABASE_URL",
    "postgres://ferris:ferris@127.0.0.1:5432/ferris_bench",
)


@asynccontextmanager
async def lifespan(app: FastAPI):
    try:
        app.state.pool = await asyncpg.create_pool(
            DATABASE_URL,
            min_size=20,
            max_size=20,
        )
    except Exception:
        # The HTTP/JSON scenario remains usable without PostgreSQL. The
        # PostgreSQL endpoint reports 503 and is rejected by the Arena.
        app.state.pool = None
    yield
    if app.state.pool is not None:
        await app.state.pool.close()


app = FastAPI(title="FastAPI Benchmark Target", lifespan=lifespan)


@app.get("/health")
async def health():
    return {"status": "ok", "framework": "FastAPI (Python)", "port": 8000}


@app.get("/api/v1/health")
async def api_health():
    return {"status": "ok", "framework": "FastAPI (Python)", "port": 8000}


@app.get("/api/v1/plain")
async def plain():
    return {
        "message": "FerrisTelemetry benchmark payload",
        "source": "memory",
        "values": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    }


@app.get("/api/v1/users")
async def get_users():
    pool = app.state.pool
    if pool is None:
        raise HTTPException(status_code=503, detail="PostgreSQL unavailable")

    try:
        rows = await pool.fetch(
            "SELECT id, username, email, role FROM users ORDER BY id LIMIT 10"
        )
    except Exception as exc:
        raise HTTPException(status_code=503, detail="PostgreSQL query failed") from exc

    users = [dict(row) for row in rows]
    return {"users": users, "source": "postgres", "count": len(users)}


@app.get("/api/v1/user")
async def get_user(id: int):
    if id < 1:
        raise HTTPException(status_code=400, detail="id must be a positive integer")
    pool = app.state.pool
    if pool is None:
        raise HTTPException(status_code=503, detail="PostgreSQL unavailable")
    try:
        row = await pool.fetchrow(
            "SELECT id, username, email, role FROM users WHERE id = $1", id
        )
    except Exception as exc:
        raise HTTPException(status_code=503, detail="PostgreSQL query failed") from exc
    if row is None:
        raise HTTPException(status_code=404, detail="user not found")
    return {"user": dict(row), "source": "postgres"}


@app.get("/api/v1/queries")
async def get_queries(ids: str):
    try:
        values = [int(value.strip()) for value in ids.split(",")]
    except ValueError as exc:
        raise HTTPException(
            status_code=400,
            detail="ids must be comma-separated positive integers",
        ) from exc
    if not 1 <= len(values) <= 100 or any(value < 1 for value in values):
        raise HTTPException(
            status_code=400,
            detail="ids must contain between 1 and 100 positive integers",
        )
    pool = app.state.pool
    if pool is None:
        raise HTTPException(status_code=503, detail="PostgreSQL unavailable")
    try:
        rows = await pool.fetch(
            "SELECT id, username, email, role FROM users "
            "WHERE id = ANY($1::bigint[]) ORDER BY id",
            values,
        )
    except Exception as exc:
        raise HTTPException(status_code=503, detail="PostgreSQL query failed") from exc
    users = [dict(row) for row in rows]
    return {"users": users, "source": "postgres", "count": len(users)}


@app.api_route("/api/v1/write", methods=["GET", "POST"])
async def write_user(id: int):
    if id < 1:
        raise HTTPException(status_code=400, detail="id must be a positive integer")
    pool = app.state.pool
    if pool is None:
        raise HTTPException(status_code=503, detail="PostgreSQL unavailable")
    try:
        row = await pool.fetchrow(
            "INSERT INTO benchmark_writes (id, value) VALUES ($1, 'benchmark') "
            "ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value, "
            "updated_at = NOW() RETURNING id",
            id,
        )
    except Exception as exc:
        raise HTTPException(status_code=503, detail="PostgreSQL write failed") from exc
    return {"id": row["id"], "source": "postgres", "written": True}


@app.get("/api/v1/compute")
async def compute():
    total = sum(i * i for i in range(1000))
    return {"result": total, "framework": "FastAPI"}


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(app, host="0.0.0.0", port=8000, log_level="warning")
