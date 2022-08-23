//https://github.com/kidanger/ipol-demorunner/blob/master/src/compilation.rs
#[macro_use]
extern crate rocket;
use rocket::fairing::AdHoc;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::request::FromParam;
use rocket::tokio::fs::File;
use rocket::{State, Config};
use std::fs;

use image::io::Reader as ImageReader;
use rand::{self, Rng};

use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use chrono::Utc;

use rocket_db_pools::sqlx;
use rocket_db_pools::{Connection, Database};
use sqlx::Row;

use rocket::serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct AppConfig {
    upload_directory : String
}
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
        Self { id }
    }
    pub fn file_path(&self) -> PathBuf {
        let root = "./upload";
        Path::new(root).join(&self.id)
    }
}
impl<'a> FromParam<'a> for ImageId {
    type Error = &'a str;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        param
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
            .then(|| Self { id: param.into() })
            .ok_or(param)
    }
}

fn is_token_valid(_token: &str) -> bool {
    true
}
#[get("/get/<token>/<id>")]
async fn get(mut db: Connection<Canard>, token: &str, id: ImageId) -> Option<File> {
    if !is_token_valid(token) {
        return None;
    }
    let now = Utc::now().timestamp();
    let db_entry = sqlx::query("SELECT expiration_date FROM images WHERE id = $1")
        .bind(&id.id)
        .fetch_one(&mut *db)
        .await;
    if let Ok(row) = db_entry {
        let expiration_date = row.get::<i64, &str>("expiration_date");
        if expiration_date <= now {
            clean_expired_images(db).await;
            return None;
        }
    } else {
        return None;
    }
    File::open(id.file_path()).await.ok()
}
#[get("/clean")]
async fn clean(db: Connection<Canard>) -> Option<File> {
    clean_expired_images(db).await;
    None
}
async fn clean_expired_images(mut db: Connection<Canard>) {
    let now = Utc::now().timestamp();
    let expired_rows = sqlx::query("SELECT id FROM images WHERE expiration_date < $1")
        .bind(&now)
        .fetch_all(&mut **db)
        .await;
    if let Ok(expired_rows) = expired_rows {
        for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
            let id = ImageId { id: id.to_string() };
            let deleted = fs::remove_file(id.file_path());
            if let Err(err) = deleted {
                eprintln!("Cannot delete {:?}", err);
            }
        }
        sqlx::query("DELETE FROM images WHERE expiration_date < $1")
            .bind(&now)
            .execute(&mut **db)
            .await
            .unwrap();
    }
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
    rocket_config: &Config, app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    token: &str,
    mut img: Form<Upload<'_>>,
) -> std::io::Result<String> {
    eprintln!("{:#?}\n{:#?}", app_config, rocket_config);
    let id = ImageId::new(ID_LENGTH);
    if !is_token_valid(token) {
        return Err(Error::new(ErrorKind::PermissionDenied, "Token not valid"));
    }

    let now = Utc::now().timestamp();
    let expiration = img.duration.map_or(i64::MAX - 1, |duration| now + duration);
    if expiration < now {
        return Err(Error::new(ErrorKind::Other, "Already expired"));
    }
    if let Some(image_path) = img.upload.path() {
        let img = ImageReader::open(image_path);
        if let Ok(img) = img {
            let dec = img.with_guessed_format();
            if let Ok(dec) = dec {
                let dec = dec.decode();
                if dec.is_err() {
                    eprintln!("{:?}", dec);
                    return Err(Error::new(ErrorKind::Other, "Image is not image"));
                }
            } else {
                return Err(Error::new(ErrorKind::Other, "Couldn't guess format"));
            }
        } else {
            return Err(Error::new(ErrorKind::Other, "Cannot access image"));
        }
    } else {
        return Err(Error::new(
            ErrorKind::Other,
            "Image is not fully downloaded",
        ));
    }

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

        Ok(id.id)
    }
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    let _r = rocket::build()
        .attach(Canard::init())
        .attach(AdHoc::config::<AppConfig>())
		.attach(AdHoc::try_on_ignite("Database Initialization", |rocket| async {
			let conn = match Canard::fetch(&rocket) {
				Some(pool) => pool.clone(), // clone the wrapped pool
				None => return Err(rocket),
			};

            let expired_rows = sqlx::query("SELECT id, expiration_date, token_used FROM images")
                .fetch_all(&**conn)
                .await;
            if expired_rows.is_err() {
                eprintln!("Initializing Database");
                sqlx::query(
                    "CREATE TABLE images (id TEXT, expiration_date UNSIGNED BIG INT, token_used TEXT);",
                )
                .execute(&**conn)
                .await
                .unwrap();
            }
			Ok(rocket)
		}))
		.attach(AdHoc::try_on_ignite("Directory Initialization", |rocket| async {
            if let Some(app_config) = rocket.state::<AppConfig>() {
                if !Path::new(&app_config.upload_directory).exists() {
                    let creation = fs::create_dir(&app_config.upload_directory);
                    if creation.is_err() {
                        panic!("The directory to store images cannot be created.");
                    }
                }
            }
			Ok(rocket)
		}))
        .mount("/", routes![get])
        .mount("/", routes![post])
        .mount("/", routes![clean])
        .launch()
        .await?;

    Ok(())
}
