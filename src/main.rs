//https://github.com/kidanger/ipol-demorunner/blob/master/src/compilation.rs
#[macro_use]
extern crate rocket;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::request::FromParam;
use rocket::tokio::fs::File;
use std::fs;

use rand::{self, Rng};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Timelike, Utc};

use sqlx::Row;
use rocket_db_pools::sqlx;
use rocket_db_pools::{Connection, Database};

use rocket::{
    http::Status,
    response::{self, Responder},
    serde::{Deserialize, Serialize},
    Request,
};

const ID_LENGTH: usize = 3;
pub struct ImageId {
    id: String,
}
impl ImageId {
    fn new(size: usize) -> Self {
        const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

        let mut id = String::with_capacity(size);
        let mut rng = rand::thread_rng();
        for _ in 0..size {
            id.push(BASE62[rng.gen::<usize>() % 62] as char);
        }
        ImageId { id }
    }
    pub fn file_path(&self) -> PathBuf {
        let root = "./upload";
        Path::new(root).join(self.id.clone())
    }
    pub fn get_id(&self) -> String {
        self.id.clone()
    }
}
impl<'a> FromParam<'a> for ImageId {
    type Error = &'a str;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        param
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
            .then(|| ImageId { id: param.into() })
            .ok_or(param)
    }
}

fn is_token_valide(token: &str) -> bool {
    true
}
#[get("/get/<token>/<id>")]
async fn get(token: &str, id: ImageId) -> Option<File> {
    if !is_token_valide(token) {
        return None;
    }
    File::open(id.file_path()).await.ok()
}
#[get("/clean/<token>")]
async fn clean(mut db: Connection<Canard>, token: &str) -> Option<String> {

    if !is_token_valide(token) {
        return None;
    }
    let now = Utc::now().timestamp();
    dbg!(now);
    let expired_rows = sqlx::query(
        "SELECT id FROM images WHERE expiration_date < $1",
    )
    .bind(&now)
    .fetch_all(&mut *db)
    .await;
    if let Ok(expired_rows) = expired_rows {
        for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
            let id = ImageId { id : id.to_string() };
            let deleted = fs::remove_file(id.file_path());
            if let Err(err) = deleted {
                eprintln!("Cannot delete {:?}", err);
            }
        }
        let deleted_rows = sqlx::query(
            "DELETE FROM images WHERE expiration_date < $1",
        )
        .bind(&now)
        .execute(&mut *db)
        .await;
    }
    Some("All clean".to_string())
}
#[derive(Debug, FromForm)]
struct Upload<'f> {
    upload: TempFile<'f>,
    duration: Option<i64>,
}
#[derive(Database)]
#[database("sqlite_logs")]
struct Canard(sqlx::SqlitePool);

#[derive(Deserialize, Serialize, sqlx::FromRow)]
#[serde(crate = "rocket::serde")]
struct ImageData {
    id: String,
    expiration_date: String,
    token_used: String,
}
#[post("/post/<token>", data = "<img>")]
async fn post(
    mut db: Connection<Canard>,
    token: &str,
    mut img: Form<Upload<'_>>,
) -> std::io::Result<String> {
    let id = ImageId::new(ID_LENGTH);
    if !is_token_valide(token) {
        return Err(Error::new(ErrorKind::PermissionDenied, "Token not valid"));
    }

    let expiration = if let Some(duration) = img.duration {
        let now = Utc::now().timestamp();
        now + duration
    } else {
        i64::MAX - 1
    };

    let added_task = sqlx::query(
        "INSERT INTO images (id, expiration_date, token_used) VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(&id.id)
    .bind(&expiration)
    .bind(&token)
    .execute(&mut *db)
    .await;

    if added_task.is_err() {
        Err(Error::new(ErrorKind::Other, "Database unavailable"))
    } else {
        img.upload.copy_to(id.file_path()).await?;

        Ok("All good buddy".to_string())
    }
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .attach(Canard::init())
        .mount("/", routes![get])
        .mount("/", routes![post])
        .mount("/", routes![clean])
}
