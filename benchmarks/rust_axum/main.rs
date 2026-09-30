use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_postgres::{Client, NoTls, Statement};

const USERS_QUERY: &str = "SELECT id, username, email, role FROM users ORDER BY id LIMIT 10";

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
}

#[derive(Clone)]
pub struct PgPool {
    connections: Arc<Vec<Arc<PgConnection>>>,
    next: Arc<AtomicUsize>,
}

struct PgConnection {
    client: Client,
    users_statement: Statement,
    user_statement: Statement,
    multi_statement: Statement,
    write_statement: Statement,
}

impl PgPool {
    async fn connect(database_url: &str, size: usize) -> Result<Self, tokio_postgres::Error> {
        let mut connections = Vec::with_capacity(size);
        for _ in 0..size {
            let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
            tokio::spawn(async move {
                if let Err(error) = connection.await {
                    eprintln!("PostgreSQL connection error: {error}");
                }
            });
            let users_statement = client.prepare(USERS_QUERY).await?;
            let user_statement = client
                .prepare("SELECT id, username, email, role FROM users WHERE id = $1")
                .await?;
            let multi_statement = client
                .prepare(
                    "SELECT id, username, email, role FROM users WHERE id = ANY($1::bigint[]) ORDER BY id",
                )
                .await?;
            let write_statement = client
                .prepare(
                    "INSERT INTO benchmark_writes (id, value) VALUES ($1, 'benchmark') \
                     ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW() \
                     RETURNING id",
                )
                .await?;
            connections.push(Arc::new(PgConnection {
                client,
                users_statement,
                user_statement,
                multi_statement,
                write_statement,
            }));
        }
        Ok(Self {
            connections: Arc::new(connections),
            next: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn connection(&self) -> Arc<PgConnection> {
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.connections.len();
        self.connections[index].clone()
    }
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub framework: &'static str,
    pub port: u16,
}

#[derive(Serialize)]
pub struct UsersResponse {
    pub users: Vec<User>,
    pub source: &'static str,
    pub count: usize,
}

#[derive(Deserialize)]
pub struct SingleQuery {
    pub id: i64,
}

#[derive(Deserialize)]
pub struct MultiQuery {
    pub ids: String,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub user: User,
    pub source: &'static str,
}

#[derive(Serialize)]
pub struct WriteResponse {
    pub id: i64,
    pub source: &'static str,
    pub written: bool,
}

fn user_from_row(row: &tokio_postgres::Row) -> Result<User, tokio_postgres::Error> {
    Ok(User {
        id: row.try_get(0)?,
        username: row.try_get(1)?,
        email: row.try_get(2)?,
        role: row.try_get(3)?,
    })
}

fn parse_ids(raw: &str) -> Result<Vec<i64>, &'static str> {
    let ids: Result<Vec<_>, _> = raw.split(',').map(str::parse::<i64>).collect();
    let ids = ids.map_err(|_| "ids must be comma-separated integers")?;
    if ids.is_empty() || ids.len() > 100 || ids.iter().any(|id| *id < 1) {
        return Err("ids must contain between 1 and 100 values");
    }
    Ok(ids)
}

#[derive(Serialize)]
pub struct ComputeResponse {
    pub result: i64,
    pub framework: &'static str,
}

#[derive(Serialize)]
pub struct PlainResponse {
    pub message: &'static str,
    pub source: &'static str,
    pub values: [u8; 10],
}

pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        framework: "Axum (Rust)",
        port: 3000,
    })
}

pub async fn plain_handler() -> impl IntoResponse {
    Json(PlainResponse {
        message: "FerrisTelemetry benchmark payload",
        source: "memory",
        values: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    })
}

pub async fn get_users_handler(State(pool): State<PgPool>) -> impl IntoResponse {
    let connection = pool.connection();
    let rows = connection
        .client
        .query(&connection.users_statement, &[])
        .await;

    match rows {
        Ok(rows) => {
            let mut users = Vec::with_capacity(rows.len());
            for row in rows {
                let user = match user_from_row(&row) {
                    Ok(user) => user,
                    Err(_) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": "row mapping failed" })),
                        )
                            .into_response();
                    }
                };
                users.push(user);
            }
            let count = users.len();
            Json(UsersResponse {
                users,
                source: "postgres",
                count,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_user_handler(
    Query(params): Query<SingleQuery>,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let connection = pool.connection();
    match connection
        .client
        .query_opt(&connection.user_statement, &[&params.id])
        .await
    {
        Ok(Some(row)) => match user_from_row(&row) {
            Ok(user) => Json(UserResponse {
                user,
                source: "postgres",
            })
            .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "row mapping failed" })),
            )
                .into_response(),
        },
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn get_multi_handler(
    Query(params): Query<MultiQuery>,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let ids = match parse_ids(&params.ids) {
        Ok(ids) => ids,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": error })),
            )
                .into_response();
        }
    };
    let connection = pool.connection();
    match connection
        .client
        .query(&connection.multi_statement, &[&ids])
        .await
    {
        Ok(rows) => {
            let mut users = Vec::with_capacity(rows.len());
            for row in rows {
                match user_from_row(&row) {
                    Ok(user) => users.push(user),
                    Err(_) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({ "error": "row mapping failed" })),
                        )
                            .into_response();
                    }
                }
            }
            Json(UsersResponse {
                count: users.len(),
                users,
                source: "postgres",
            })
            .into_response()
        }
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn write_handler(
    Query(params): Query<SingleQuery>,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let connection = pool.connection();
    match connection
        .client
        .query_one(&connection.write_statement, &[&params.id])
        .await
    {
        Ok(row) => match row.try_get::<_, i64>(0) {
            Ok(id) => Json(WriteResponse {
                id,
                source: "postgres",
                written: true,
            })
            .into_response(),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "write mapping failed" })),
            )
                .into_response(),
        },
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

pub async fn compute_handler() -> impl IntoResponse {
    let mut total: i64 = 0;
    for i in 0..1000i64 {
        total += i * i;
    }
    Json(ComputeResponse {
        result: total,
        framework: "Axum (Rust)",
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://ferris:ferris@127.0.0.1:5432/ferris_bench".to_string());
    // Use the same fixed, warmed 20-connection PostgreSQL pool as the other
    // benchmark targets. This binary intentionally uses the native driver so
    // framework overhead is not confused with SQLx's higher-level mapping.
    let pool = PgPool::connect(&database_url, 20).await?;

    let port = std::env::var("AXUM_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);

    // 2. Setup Router
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/plain", get(plain_handler))
        .route("/api/v1/users", get(get_users_handler))
        .route("/api/v1/user", get(get_user_handler))
        .route("/api/v1/queries", get(get_multi_handler))
        .route("/api/v1/write", get(write_handler).post(write_handler))
        .route("/api/v1/compute", get(compute_handler))
        .with_state(pool);

    // 3. Bind to port 3000 (or AXUM_PORT for isolated benchmark runs).
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    println!("INFO: Standalone Axum (Rust) server running on http://127.0.0.1:{port}");

    axum::serve(listener, app).await?;
    Ok(())
}
