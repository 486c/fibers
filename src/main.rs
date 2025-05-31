use std::sync::Arc;

use anyhow::Result;
use axum::{body::Bytes, extract::{Request, State}, routing::{get, post}, Json, RequestExt, Router};
use fibers::{auth::User, notifications::notifications_upgrade, routes::{self, users::ApiUser}, state::{FiberState, FiberStateInner}};
use serde::Serialize;
use sqlx::migrate;
use tokio::net::TcpListener;

async fn shutdown_signal() {
  tokio::signal::ctrl_c()
    .await
    .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt()
    .with_max_level(tracing::Level::INFO)
    .with_target(false)
    .with_thread_names(false)
    .init();

  let state = FiberStateInner::new().await?;
  let state = Arc::new(state);

  migrate!()
    .run(&state.pool)
    .await?;

  let listener = TcpListener::bind("0.0.0.0:19991").await?;

  let app = Router::new()
    .nest("/signalr", routes::signalr::router())
    .nest("/oauth", routes::oauth::router())
    .merge(routes::users::router(state.clone()))
    .merge(routes::beatmaps::router(state.clone()))
    // do i even need registering?
    // .route("/users", post(register))
    .route("/api/v2/notifications", get(notifications))
    .route("/api/v2/friends", get(friends))
    .route("/api/v2/chat/ack", post(chat_ack))
    //.router("/api/v2/beatmaps/")
    .route("/notifications", get(notifications_upgrade))
    .fallback(fallback_handler)
    .with_state(state);

  axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

  Ok(())
}

// #[derive(Debug, TryFromMultipart)]
// struct RegisterMultipart {
//   #[form_data(field_name = "user[username]")]
//   username: String,
//   #[form_data(field_name = "user[user_email]")]
//   email: String,
//   #[form_data(field_name = "user[password]")]
//   password: String,
// }

// async fn register(
//   user: TypedMultipart<RegisterMultipart>,
// ) {
//   println!("{:?}", user);
// }

#[derive(Default, Serialize)]
struct Notifications {
  has_more: bool,
  notifications: Vec<()>,
  unread_count: usize,
  notification_endpoint: String,
}

async fn notifications() -> Json<Notifications> {
  Json(Notifications {
    notification_endpoint: "ws://localhost:19991/notifications".into(),
    ..Default::default()
  })
}

#[derive(Serialize)]
struct Relation {
  target_id: i64,
  relation_type: u8,
  mutual: bool,
  user: ApiUser,
}

async fn friends(
  State(state): State<FiberState>,
) -> Json<Vec<Relation>> {
  let Ok(bot) = sqlx::query_as::<_, User>("select * from users where id = 1")
    .fetch_one(&state.pool)
    .await else
  {
    unreachable!("probably");
  };

  Json(vec![
    Relation {
      target_id: 1,
      relation_type: 0,
      mutual: true,
      user: ApiUser::new(&bot)
    }
  ])
}

#[derive(Default, Serialize)]
struct ChatAck {
  silences: Vec<()>,
}

async fn chat_ack() -> Json<ChatAck> {
  Json(ChatAck::default())
}

async fn fallback_handler(
  req: Request
) {
  tracing::debug!("[{}] {}", req.method(), req.uri());
  tracing::debug!("Headers: {:?}", req.headers());

  if let Ok(bytes) = req.extract::<Bytes, _>().await {
    tracing::debug!("Body: {:?}", bytes);
  }
}

