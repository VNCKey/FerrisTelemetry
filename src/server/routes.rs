use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::db::{self, User};

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub framework: &'static str,
    pub port: u16,
    pub engine: &'static str,
}

#[derive(Serialize)]
pub struct UsersResponse {
    pub users: Vec<User>,
    pub source: &'static str,
    pub count: usize,
}

#[derive(Serialize)]
pub struct ComputeResponse {
    pub result: i64,
    pub framework: &'static str,
}

#[derive(Serialize)]
pub struct OrderItem {
    pub order_id: &'static str,
    pub item: &'static str,
    pub price: f64,
    pub quantity: u32,
    pub status: &'static str,
}

#[derive(Deserialize)]
pub struct CreateUserPayload {
    pub username: String,
    pub email: String,
    pub role: String,
}

pub async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        framework: "Axum (Rust)",
        port: 3000,
        engine: "Tokio Async Engine",
    })
}

pub async fn get_users_handler(State(pool): State<SqlitePool>) -> impl IntoResponse {
    match db::get_users(&pool).await {
        Ok(users) => {
            let count = users.len();
            Json(UsersResponse {
                users,
                source: "sqlite",
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

pub async fn get_orders_handler() -> impl IntoResponse {
    let sample_orders = vec![
        OrderItem {
            order_id: "ORD-9021",
            item: "High-Performance Rust Handbook",
            price: 49.99,
            quantity: 2,
            status: "delivered",
        },
        OrderItem {
            order_id: "ORD-9022",
            item: "Tokio Async Concurrency Guide",
            price: 39.50,
            quantity: 1,
            status: "processing",
        },
        OrderItem {
            order_id: "ORD-9023",
            item: "Ferris Plushie Limited Edition",
            price: 29.99,
            quantity: 3,
            status: "shipped",
        },
    ];

    Json(serde_json::json!({
        "orders": sample_orders,
        "count": 3,
        "framework": "Axum"
    }))
}

pub async fn create_user_handler(
    State(pool): State<SqlitePool>,
    Json(payload): Json<CreateUserPayload>,
) -> impl IntoResponse {
    let res = sqlx::query("INSERT INTO users (username, email, role) VALUES (?, ?, ?)")
        .bind(&payload.username)
        .bind(&payload.email)
        .bind(&payload.role)
        .execute(&pool)
        .await;

    match res {
        Ok(_) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "status": "created", "username": payload.username })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
