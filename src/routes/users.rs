use std::{fmt::Display, str::FromStr};

use axum::{body::Bytes, extract::{Multipart, Path, State}, middleware, routing::{get, post, put}, Extension, Form, Json, Router};
use axum_typed_multipart::{TryFromMultipart, TypedMultipart};
use chrono::Utc;
use rosu_mods::{GameMod, GameMods};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;
use sqlx::prelude::FromRow;

use crate::{auth::{self, User}, osu_mods::{OsuModLazer, OsuModsLazer}, state::FiberState};

#[derive(Serialize)]
pub struct ApiUser {
  avatar_url: String,
  country_code: String,
  id: u32,
  is_active: bool,
  is_bot: bool,
  is_deleted: bool,
  is_online: bool,
  is_supporter: bool,
  last_visit: Option<String>,
  pm_friends_only: bool,
  username: String,
  cover_url: String,
  has_supported: bool,
  join_date: String,
  session_verified: bool,
  statistics_rulesets: Option<StatisticsRulesets>,
}

#[derive(Serialize)]
struct StatisticsRulesets {
  osu: Statistics,
  taiko: Statistics,
  fruits: Statistics,
  mania: Statistics,
}

#[derive(Serialize)]
struct Statistics {
  level: StatisticsLevel,
  is_ranked: bool,
  global_rank: u32,
  country_rank: u32,
  pp: f32,
  ranked_score: u32,
  accuracy: f32,
  play_count: u32,
  play_time: u32,
  total_score: u32,
  total_hits: u32,
  maximum_combo: u32,
  replays_watched_by_others: u32,
  grade_counts: StatisticsGrades
}

impl Statistics {
  fn new(stats: &DbStatistics) -> Self {
    Self {
      level: StatisticsLevel {
        current: stats.level.floor() as u16,
        progress: stats.level.fract(),
      },
      is_ranked: true,
      global_rank: 1,
      country_rank: 1,
      pp: stats.pp,
      ranked_score: 0,
      accuracy: 0.,
      play_count: stats.playcount,
      play_time: 0,
      total_score: 0,
      total_hits: 0,
      maximum_combo: 0,
      replays_watched_by_others: 0,
      grade_counts: StatisticsGrades {
        ssh: stats.ssh,
        ss: stats.ss,
        sh: stats.sh,
        s: stats.s,
        a: stats.a,
      }
    }
  }
}

#[derive(Serialize)]
struct StatisticsLevel {
  current: u16,
  progress: f32,
}

#[derive(Serialize)]
struct StatisticsGrades {
  ssh: u32,
  ss: u32,
  sh: u32,
  s: u32,
  a: u32,
}

impl ApiUser {
  pub fn new(user: &User) -> Self {
    Self {
      avatar_url: "https://f.octo.moe/files/ddd854e741d78037.png".into(), // ass pic
      country_code: "JP".into(),
      id: user.id as u32,
      is_active: true,
      is_bot: true,
      is_deleted: false,
      is_online: true,
      is_supporter: true,
      last_visit: None,
      pm_friends_only: false,
      username: user.username.clone(),

      cover_url: "https://f.octo.moe/files/0c5ea11c11244cc2.jpg".into(),
      has_supported: true,
      join_date: user.joined_at.to_rfc3339(),

      session_verified: true,
      statistics_rulesets: None,
    }
  }

  fn with_statistics(mut self, statistics: StatisticsRulesets) -> Self {
    self.statistics_rulesets = Some(statistics);

    self
  }
}

async fn me(
  Extension(user): Extension<User>,
) -> Json<ApiUser> {
  Json(ApiUser::new(&user))
}

#[derive(Clone, FromRow)]
struct DbStatistics {
  user_id: i64,
  ruleset_id: u32,
  level: f32,
  pp: f32,
  playcount: u32,
  ssh: u32,
  ss: u32,
  sh: u32,
  s: u32,
  a: u32
}

impl DbStatistics {
  fn new(user_id: i64, ruleset_id: u32) -> Self {
    Self {
      user_id,
      ruleset_id,
      level: 0.,
      pp: 0.,
      playcount: 0,
      ssh: 0,
      ss: 0,
      sh: 0,
      s: 0,
      a: 0,
    }
  }
}

async fn get_user(
  State(state): State<FiberState>,
  Path(id): Path<i64>,
) -> Json<ApiUser> {
  let user = sqlx::query_as::<_, User>(r#"
    select * from users
    where id = ?
  "#)
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .unwrap();
  // let user = sqlx::query_as::<_, User>(r#"
  //   select * from users
  //   where id = ?
  // "#)
  //   .bind(id)
  //   .fetch_one(&state.pool)
  //   .await
  //   .unwrap();

   let stats = sqlx::query_as::<_, DbStatistics>(r#"
     select * from statistics
     where user_id = ?
   "#)
     .bind(id)
     .fetch_all(&state.pool)
     .await
     .unwrap();

  let stats = StatisticsRulesets {    
    osu: Statistics::new(&stats.iter().find(|s| s.ruleset_id == 0)
      .cloned()
      .unwrap_or_else(|| DbStatistics::new(user.id, 0))),
    taiko: Statistics::new(&stats.iter().find(|s| s.ruleset_id == 1)
      .cloned()
      .unwrap_or_else(|| DbStatistics::new(user.id, 1))),
    fruits: Statistics::new(&stats.iter().find(|s| s.ruleset_id == 2)
      .cloned()
      .unwrap_or_else(|| DbStatistics::new(user.id, 2))),
    mania: Statistics::new(&stats.iter().find(|s| s.ruleset_id == 3)
      .cloned()
      .unwrap_or_else(|| DbStatistics::new(user.id, 3))),
  };

  let response = ApiUser::new(&user)
    .with_statistics(stats);

  Json(response)
}

pub fn router(state: FiberState) -> Router<FiberState> {
  Router::new()
    .route("/api/v2/me/", get(me))
    .route("/api/v2/users/{id}/", get(get_user))
    .layer(middleware::from_fn_with_state(state, auth::middleware))
}
