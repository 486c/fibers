use axum::{body::Bytes, extract::{Path, State}, middleware, routing::{get, post, put}, Extension, Json, Router};
use axum_typed_multipart::{TryFromMultipart, TypedMultipart};
use chrono::Utc;
use serde::{Deserialize, Serialize, Serializer};

use crate::{auth::{self, User}, osu_mods::OsuModLazer, state::FiberState};


// HACK: Hacky as hell, will leave for now, maybe make it more
// generalized in the near future or think how to handle
// it bit better
fn serialize_option_as_zero<S>(value: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(v) => serializer.serialize_u32(*v),
        None => serializer.serialize_u32(0),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SoloScoresSubmitStatistic {
    #[serde(serialize_with = "serialize_option_as_zero")]
    perfect: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    great: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    ok: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    meh: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    miss: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    large_tick_miss: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    large_tick_hit: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    ignore_miss: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    ignore_hit: Option<u32>,

    #[serde(serialize_with = "serialize_option_as_zero")]
    slider_tail_hit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SoloScoresSubmit {
    ruleset_id: u8,
    passed: bool,
    total_score: u64,
    total_score_without_mods: u64,
    accuracy: f32,
    max_combo: u32,
    rank: String,
    mods: Option<Vec<OsuModLazer>>,
    ranked: bool,
    statistics: SoloScoresSubmitStatistic,
    maximum_statistics: SoloScoresSubmitStatistic,

    // When submitting scores those are 
    // two parameters that are filled
    // after score was successfully submitted/processed by the server
    id: Option<i64>,
    // position: Option<i64>
}

#[derive(sqlx::FromRow)]
struct ScoreDb {
    online_id: i64,
    data_json: Vec<u8>,
}

async fn put_solo_scores(
    Extension(user): Extension<User>,
    State(state): State<FiberState>,
    Path((beatmap_id, token)): Path<(i64, i64)>,
    body: Bytes,
) -> Json<SoloScoresSubmit> {
    // Check if this score is indeed in flight
    let lock = state.in_flight_scores.read().await;

    if !lock.contains(&token) {
        // TODO: A case when we got score submission without
        // any token retrival
        unimplemented!();
    }
    drop(lock);

    let mut lock = state.in_flight_scores.write().await;
    lock.remove(&token);
    drop(lock);

    // Handle storing and returning back to the client

    // Currently that's a some sort of verifier before inserting to the
    // database
    // TODO: Think about make it a bit efficient?
    let _payload: SoloScoresSubmit = serde_json::from_slice(&body).unwrap();


    let payload_str = str::from_utf8(&body).unwrap();
    
    // Insert to retrieve online_id
    let record = sqlx::query!(
        "INSERT INTO scores (data_json, beatmap_id, user_id) VALUES (?1, ?2, ?3) RETURNING *",
        payload_str, beatmap_id, user.id
    )
    .fetch_one(&state.pool)
    .await
    .unwrap();

    let mut payload_from_db: SoloScoresSubmit = serde_json::from_slice(&record.data_json).unwrap();
    
    payload_from_db.id = Some(record.online_id);

    tracing::info!(user = user.id, bid = beatmap_id, "Submitted score!");

    Json(payload_from_db)
}

#[derive(Debug, TryFromMultipart)]
struct SoloScoresTokenSubmit {
    version_hash: String,
    beatmap_hash: String,
    ruleset_id: i32
}

/// Assign a token for the score submission
///
/// Expected response -> `{ id: long }`
async fn solo_score_token(
  Extension(user): Extension<User>,
  State(state): State<FiberState>,
  Path(beatmap_id): Path<i64>,
  _multipart: TypedMultipart<SoloScoresTokenSubmit>,
) -> Json<serde_json::Value> {
    tracing::info!(user = user.id, "Received score token retrival");

    let token: i64 = rand::random();
    
    {
        let mut lock = state.in_flight_scores.write().await;
        lock.insert(token);
    }

    Json(serde_json::json!{{
        "id": token
    }})
}

// TODO: Did this structure cuz i'm lazy and wanna test it ASAP
// make it more reusable somewhere in the future
#[derive(Debug, Serialize)]
pub struct LeadboardScoreUser {
    id: i64,
    username: String,
    country_code: String,
    avatar_url: String,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardScoreEntry {
    /// Online score id
    id: i64,
    user_id: i64,
    max_combo: u32,
    score: u64,
    rank: String,
    pp: f32,
    replay: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    mode: String,
    //mode_int: String,
    mods: Vec<OsuModLazer>, // Should never be NULL json
    statistics: SoloScoresSubmitStatistic,
    user: LeadboardScoreUser,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardScoreResponse {
    scores: Vec<LeaderboardScoreEntry>,

    // TODO: Self score of the user
    //user_score: Option<>
}

async fn get_scores(
    Extension(user): Extension<User>,
    State(state): State<FiberState>,
    Path(beatmap_id): Path<i64>,
) -> Json<LeaderboardScoreResponse> {
    tracing::info!(user = user.id, bid = beatmap_id, "Leaderboard scores request");

    // TODO Whole handler is ugly AF i know...

    let mut scores = Vec::new();

    let records = sqlx::query!(
        "SELECT * FROM scores WHERE user_id = ?1 AND beatmap_id = ?2",
        user.id, beatmap_id
    )
    .fetch_all(&state.pool)
    .await
    .unwrap();

    for record in records {
        let score_json: SoloScoresSubmit = serde_json::from_slice(&record.data_json).unwrap();
        let user = sqlx::query!("SELECT * FROM users WHERE id = ?1", record.user_id)
            .fetch_one(&state.pool)
            .await
            .unwrap();

        let score = LeaderboardScoreEntry {
            id: record.online_id,
            user_id: record.user_id,
            max_combo: score_json.max_combo,
            rank: score_json.rank.clone(),
            pp: 727.0,
            replay: false,
            created_at: Utc::now(),
            mode: "osu".to_owned(),
            mods: score_json.mods.clone().unwrap_or(Vec::new()),
            statistics: score_json.statistics.clone(),
            user: LeadboardScoreUser {
                id: user.id,
                username: user.username.clone(),
                country_code: "JP".to_owned(),
                avatar_url: "https://f.octo.moe/files/ddd854e741d78037.png".to_owned(),
            },
            score: score_json.total_score,
        };

        scores.push(score);
    }

    let res = LeaderboardScoreResponse { scores };

    Json(res)
}

pub fn router(state: FiberState) -> Router<FiberState> {
  Router::new()
    .route("/api/v2/beatmaps/{beatmap_id}/solo/scores", post(solo_score_token))
    .route("/api/v2/beatmaps/{beatmap_id}/solo/scores/{token}", put(put_solo_scores))
    .route("/api/v2/beatmaps/{beatmap_id}/scores", get(get_scores))
    .layer(middleware::from_fn_with_state(state, auth::middleware))
}
