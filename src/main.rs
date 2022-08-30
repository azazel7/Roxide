//https://github.com/kidanger/ipol-demorunner/blob/master/src/compilation.rs
#[macro_use]
extern crate rocket;
mod file_id;
mod user;

use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;

use rocket::fairing::AdHoc;
use rocket::local::blocking::Client;
use rocket::response::Responder;
use rocket::serde::Deserialize;
use rocket::config::Config;

use rocket_db_pools::sqlx;
use rocket_db_pools::Database;

use sqlx::Row;
use sqlx::SqlitePool;

use crate::file_id::FileId;

/// Error for roxide, returned as much as possible
#[derive(Debug, thiserror::Error)]
enum RoxideError {
    #[error("roxide : {0}")]
    Roxide(String),
    #[error("rocket : {0}")]
    Rocket(#[from] rocket::Error),
    #[error("database : {0}")]
    Database(#[from] sqlx::error::Error),
    #[error("IO : {0}")]
    IO(#[from] std::io::Error),
}

/// Implement Responder for RoxideError so it can be returned by Rocket.
///
/// The function simply return the to_string of the error.
impl<'r> Responder<'r, 'static> for RoxideError {
    fn respond_to(self, req: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
        let string = self.to_string();
        rocket::Response::build_from(string.respond_to(req)?)
            .status(rocket::http::Status::InternalServerError)
            .ok()
    }
}

/// Structure that contains the configuration of Roxide.
///
/// This configuration is extracted from Rocket.toml.
#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct AppConfig {
    upload_directory: String,
    id_length: usize,
    max_upload: usize,
    cleaning_frequency: usize,
    url : String,
}

/// Type that encapsulate a connection to the database
#[derive(Database)]
#[database("sqlite_logs")]
struct Canard(sqlx::SqlitePool);

/// Function that check if a token is valid.
fn is_token_valid(_token: &str) -> bool {
    true
}

#[rocket::main]
async fn main() -> Result<(), RoxideError> {
    let mut r = rocket::build();

    r = r.attach(Canard::init())
        .attach(AdHoc::config::<AppConfig>())
		.attach(AdHoc::try_on_ignite("Database Initialization", |rocket| async {
			let conn = match Canard::fetch(&rocket) {
				Some(pool) => pool.clone(), // clone the wrapped pool
				None => return Err(rocket),
			};

            let expired_rows = sqlx::query("SELECT id, title, expiration_date, upload_date, token_used, content_type, download_count, public, size FROM files")
                .fetch_all(&**conn)
                .await;
            if expired_rows.is_err() {
                eprintln!("Initializing Database");
                let create = sqlx::query(
                    "CREATE TABLE files (id TEXT, title TEXT, expiration_date UNSIGNED BIG INT, upload_date UNSIGNED BIG INT, token_used TEXT, content_type TEXT, download_count UNSIGNED BIG INT, public BOOL, size UNSIGNED BIG INT);",
                )
                .execute(&**conn)
                .await;
                if create.is_err() {
                    return Err(rocket);
                }
            }
			Ok(rocket)
		}))
		.attach(AdHoc::try_on_ignite("Directory Initialization", |rocket| async {
            if let Some(app_config) = rocket.state::<AppConfig>() {
                if !Path::new(&app_config.upload_directory).exists() {
                    let creation = fs::create_dir(&app_config.upload_directory);
                    if creation.is_err() {
                        panic!("The directory to store files cannot be created.");
                    }
                }
            }
			Ok(rocket)
		}))
        .attach(AdHoc::on_liftoff("Database Cleanning", |rocket| {
            Box::pin(async move {
                let conn = match Canard::fetch(rocket) {
                    Some(pool) => pool.clone(), // clone the wrapped pool
                    None => panic!("Cannot fetch database"),
                };
                let now = Utc::now().timestamp();
                let expired_rows = sqlx::query("SELECT id FROM files WHERE expiration_date < $1")
                    .bind(&now)
                    .fetch_all(&**conn)
                    .await;
                if let Ok(expired_rows) = expired_rows {
                    for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
                        let id = FileId::from(id);
                        let deleted = fs::remove_file(id.file_path("./upload"));
                        if let Err(err) = deleted {
                            eprintln!("Cannot delete {:?}", err);
                        }
                    }
                    sqlx::query("DELETE FROM files WHERE expiration_date < $1")
                        .bind(&now)
                        .execute(&**conn)
                        .await
                        .unwrap();
                }
            })
        }))
        .attach(user::stage());

    let r = r.ignite().await?;


    let app_config = Config::figment().extract::<AppConfig>().unwrap();
    let cleaning_frequency = app_config.cleaning_frequency as u64;
    let upload_directory = app_config.upload_directory.to_string();

    rocket::tokio::task::spawn(async move {
        let conn = SqlitePool::connect("database.sqlite").await.unwrap();
        loop {
            rocket::tokio::time::sleep(Duration::from_secs(cleaning_frequency)).await;
            let now = Utc::now().timestamp();
            let expired_rows = sqlx::query("SELECT id FROM files WHERE expiration_date < $1")
                .bind(&now)
                .fetch_all(&conn)
                .await;
            if let Ok(expired_rows) = expired_rows {
                for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
                    let id = FileId::from(id);
                    let deleted = fs::remove_file(id.file_path(&upload_directory));
                    if let Err(err) = deleted {
                        eprintln!("Cannot delete {:?}", err);
                    }
                }
                sqlx::query("DELETE FROM files WHERE expiration_date < $1")
                    .bind(&now)
                    .execute(&conn)
                    .await
                    .unwrap();
            }
        }
    });

    let _ = r.launch().await?;


    Ok(())
}
