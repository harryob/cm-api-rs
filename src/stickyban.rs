use core::panic;
use std::collections::HashSet;

use rocket::{http::Status, serde::json::Json, State};
use rocket_db_pools::Connection;
use serde::Serialize;
use sqlx::{prelude::FromRow, query, query_as, MySqlConnection};

use crate::{
    admin::Admin,
    logging::log_external,
    player::{create_note, get_player_ckey, get_player_id},
    Cmdb, Config,
};

#[derive(Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Stickyban {
    id: i32,
    identifier: String,
    reason: String,
    message: String,
    date: String,
    active: i32,

    #[sqlx(rename = "adminid")]
    admin_id: Option<i64>,

    #[sqlx(skip)]
    admin_ckey: Option<String>,
}

#[get("/")]
pub async fn all_stickybans(mut db: Connection<Cmdb>) -> Json<Vec<Stickyban>> {
    let query_result: Result<Vec<Stickyban>, sqlx::Error> = query_as("SELECT * FROM stickyban")
        .fetch_all(&mut **db)
        .await;

    let mut query = match query_result {
        Ok(result) => result,
        Err(err) => panic!("{}", err),
    };

    for stickyban in &mut query {
        if stickyban.admin_id.is_some() {
            stickyban.admin_ckey = get_player_ckey(&mut db, stickyban.admin_id.unwrap()).await;
        }
    }

    Json(query)
}

#[post("/Whitelist?<ckey>")]
pub async fn whitelist(
    mut db: Connection<Cmdb>,
    ckey: String,
    admin: Admin,
    config: &State<Config>,
) -> Status {
    let admin_id = match get_player_id(&mut db, &admin.username).await {
        Some(admin_id) => admin_id,
        None => return Status::Unauthorized,
    };

    let player_id = match get_player_id(&mut db, &ckey).await {
        Some(player_id) => player_id,
        None => return Status::BadRequest,
    };

    let query = match query(
        "UPDATE stickyban_matched_ckey SET whitelisted = 1 WHERE ckey = ? AND whitelisted = 0",
    )
    .bind(&ckey)
    .execute(&mut **db)
    .await
    {
        Ok(query) => query,
        Err(_) => return Status::Forbidden,
    };

    if query.rows_affected() > 0 {
        create_note(
            &mut db,
            player_id,
            admin_id,
            &"User was whitelisted against all stickybans.".to_string(),
            true,
            1,
        )
        .await;
        let _ = log_external(
            config,
            "Player Whitelisted".to_string(),
            format!(
                "{} whitelisted {} against all matching stickybans.",
                &admin.username, &ckey
            ),
        )
        .await;
    };

    Status::Accepted
}

trait StickybanMatch: Send + Sync {
    fn get_parent_id(&self) -> i64;
}

#[derive(FromRow, Serialize)]
pub struct StickybanMatchedCid {
    id: i64,
    cid: String,
    linked_stickyban: i64,
}

impl StickybanMatch for StickybanMatchedCid {
    fn get_parent_id(&self) -> i64 {
        self.linked_stickyban
    }
}

#[get("/<id>/Match/Cid")]
pub async fn get_matched_cids(mut db: Connection<Cmdb>, id: i64) -> Json<Vec<StickybanMatchedCid>> {
    match query_as("SELECT * FROM stickyban_matched_cid WHERE linked_stickyban = ?")
        .bind(id)
        .fetch_all(&mut **db)
        .await
    {
        Ok(result) => Json(result),
        Err(_) => Json(Vec::new()),
    }
}

#[get("/Cid?<cid>")]
pub async fn get_all_cid(mut db: Connection<Cmdb>, cid: String) -> Json<Vec<Stickyban>> {
    let query: Vec<StickybanMatchedCid> =
        match query_as("SELECT * FROM stickyban_matched_cid WHERE cid = ?")
            .bind(cid)
            .fetch_all(&mut **db)
            .await
        {
            Ok(result) => result,
            Err(err) => panic!("{}", err),
        };

    let mut unique_sticky: HashSet<String> = HashSet::new();

    for sticky in &query {
        unique_sticky.insert(sticky.linked_stickyban.to_string());
    }

    Json(
        get_stickybans_by_ids(
            &mut db,
            &query.into_iter().map(|s| Box::new(s) as _).collect(),
        )
        .await,
    )
}

#[derive(FromRow, Serialize)]
pub struct StickybanMatchedCkey {
    id: i64,
    ckey: Option<String>, // somehow?
    linked_stickyban: i64,
    whitelisted: i32,
}

impl StickybanMatch for StickybanMatchedCkey {
    fn get_parent_id(&self) -> i64 {
        self.linked_stickyban
    }
}

#[get("/<id>/Match/Ckey")]
pub async fn get_matched_ckey(
    mut db: Connection<Cmdb>,
    id: i64,
) -> Json<Vec<StickybanMatchedCkey>> {
    match query_as(
        "SELECT * FROM stickyban_matched_ckey WHERE linked_stickyban = ? AND whitelisted = 0",
    )
    .bind(id)
    .fetch_all(&mut **db)
    .await
    {
        Ok(result) => Json(result),
        Err(err) => panic!("{}", err),
    }
}

#[get("/Ckey?<ckey>")]
pub async fn get_all_ckey(mut db: Connection<Cmdb>, ckey: String) -> Json<Vec<Stickyban>> {
    let query: Vec<StickybanMatchedCkey> =
        match query_as("SELECT * FROM stickyban_matched_ckey WHERE ckey = ? AND whitelisted = 0")
            .bind(ckey)
            .fetch_all(&mut **db)
            .await
        {
            Ok(result) => result,
            Err(err) => panic!("{}", err),
        };

    Json(
        get_stickybans_by_ids(
            &mut db,
            &query.into_iter().map(|s| Box::new(s) as _).collect(),
        )
        .await,
    )
}

#[derive(FromRow, Serialize)]
pub struct StickybanMatchedIp {
    id: i64,
    ip: String,
    linked_stickyban: i64,
}

impl StickybanMatch for StickybanMatchedIp {
    fn get_parent_id(&self) -> i64 {
        self.linked_stickyban
    }
}

#[get("/<id>/Match/Ip")]
pub async fn get_matched_ip(mut db: Connection<Cmdb>, id: i64) -> Json<Vec<StickybanMatchedIp>> {
    match query_as("SELECT * FROM stickyban_matched_ip WHERE linked_stickyban = ?")
        .bind(id)
        .fetch_all(&mut **db)
        .await
    {
        Ok(result) => Json(result),
        Err(_) => Json(Vec::new()),
    }
}

#[get("/Ip?<ip>")]
pub async fn get_all_ip(mut db: Connection<Cmdb>, ip: String) -> Json<Vec<Stickyban>> {
    let query: Vec<StickybanMatchedIp> =
        match query_as("SELECT * FROM stickyban_matched_ip WHERE ip = ?")
            .bind(ip)
            .fetch_all(&mut **db)
            .await
        {
            Ok(result) => result,
            Err(err) => panic!("{}", err),
        };

    Json(
        get_stickybans_by_ids(
            &mut db,
            &query.into_iter().map(|s| Box::new(s) as _).collect(),
        )
        .await,
    )
}

async fn get_stickybans_by_ids(
    db: &mut MySqlConnection,
    ids: &Vec<Box<dyn StickybanMatch>>,
) -> Vec<Stickyban> {
    let mut unique_sticky: HashSet<String> = HashSet::new();

    for sticky in ids {
        unique_sticky.insert(sticky.get_parent_id().to_string());
    }

    match query_as("SELECT * FROM stickyban WHERE FIND_IN_SET(id, ?) AND active = 1")
        .bind(unique_sticky.into_iter().collect::<Vec<String>>().join(","))
        .fetch_all(db)
        .await
    {
        Ok(result) => result,
        Err(_) => Vec::new(),
    }
}
