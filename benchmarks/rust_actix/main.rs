use actix_web::{get, route, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
struct SingleQuery {
    id: i64,
}

#[derive(Deserialize)]
struct MultiQuery {
    ids: String,
}

#[derive(Serialize)]
struct UserResponse {
    user: User,
    source: &'static str,
}

#[derive(Serialize)]
struct WriteResponse {
    id: i64,
    source: &'static str,
    written: bool,
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

#[get("/health")]
async fn health_handler() -> impl Responder {
    web::Json(HealthResponse {
        status: "ok",
        framework: "Actix Web (Rust)",
        port: 4000,
    })
}

#[get("/api/v1/health")]
async fn api_health_handler() -> impl Responder {
    web::Json(HealthResponse {
        status: "ok",
        framework: "Actix Web (Rust)",
        port: 4000,
    })
}

#[get("/api/v1/users")]
async fn get_users_handler(pool: web::Data<PgPool>) -> impl Responder {
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
                        return HttpResponse::InternalServerError()
                            .json(serde_json::json!({ "error": "row mapping failed" }));
                    }
                };
                users.push(user);
            }
            let count = users.len();
            HttpResponse::Ok().json(UsersResponse {
                users,
                source: "postgres",
                count,
            })
        }
        Err(e) => {
            HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

#[get("/api/v1/user")]
async fn get_user_handler(
    query: web::Query<SingleQuery>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    let connection = pool.connection();
    match connection
        .client
        .query_opt(&connection.user_statement, &[&query.id])
        .await
    {
        Ok(Some(row)) => match user_from_row(&row) {
            Ok(user) => HttpResponse::Ok().json(UserResponse {
                user,
                source: "postgres",
            }),
            Err(_) => HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "row mapping failed" })),
        },
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(error) => HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": error.to_string() })),
    }
}

#[get("/api/v1/queries")]
async fn get_multi_handler(
    query: web::Query<MultiQuery>,
    pool: web::Data<PgPool>,
) -> impl Responder {
    let ids = match parse_ids(&query.ids) {
        Ok(ids) => ids,
        Err(error) => {
            return HttpResponse::BadRequest().json(serde_json::json!({ "error": error }))
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
                        return HttpResponse::InternalServerError()
                            .json(serde_json::json!({ "error": "row mapping failed" }));
                    }
                }
            }
            let count = users.len();
            HttpResponse::Ok().json(UsersResponse {
                users,
                source: "postgres",
                count,
            })
        }
        Err(error) => HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": error.to_string() })),
    }
}

#[route("/api/v1/write", method = "GET", method = "POST")]
async fn write_handler(query: web::Query<SingleQuery>, pool: web::Data<PgPool>) -> impl Responder {
    let connection = pool.connection();
    match connection
        .client
        .query_one(&connection.write_statement, &[&query.id])
        .await
    {
        Ok(row) => match row.try_get::<_, i64>(0) {
            Ok(id) => HttpResponse::Ok().json(WriteResponse {
                id,
                source: "postgres",
                written: true,
            }),
            Err(_) => HttpResponse::InternalServerError()
                .json(serde_json::json!({ "error": "write mapping failed" })),
        },
        Err(error) => HttpResponse::InternalServerError()
            .json(serde_json::json!({ "error": error.to_string() })),
    }
}

#[get("/api/v1/plain")]
async fn plain_handler() -> impl Responder {
    web::Json(PlainResponse {
        message: "FerrisTelemetry benchmark payload",
        source: "memory",
        values: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    })
}

#[get("/api/v1/compute")]
async fn compute_handler() -> impl Responder {
    let mut total: i64 = 0;
    for i in 0..1000i64 {
        total += i * i;
    }
    web::Json(ComputeResponse {
        result: total,
        framework: "Actix Web (Rust)",
    })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://ferris:ferris@127.0.0.1:5432/ferris_bench".to_string());
    // Match Axum/Fiber/Python: fixed, fully connected 20-connection pool.
    let pool = PgPool::connect(&database_url, 20)
        .await
        .map_err(std::io::Error::other)?;
    let port = std::env::var("ACTIX_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(4000);

    let pool_data = web::Data::new(pool);

    println!("INFO: Standalone Actix Web (Rust) server running on http://127.0.0.1:{port}");

    HttpServer::new(move || {
        App::new()
            .app_data(pool_data.clone())
            .service(health_handler)
            .service(api_health_handler)
            .service(plain_handler)
            .service(get_users_handler)
            .service(get_user_handler)
            .service(get_multi_handler)
            .service(write_handler)
            .service(compute_handler)
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
