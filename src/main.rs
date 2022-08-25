//https://github.com/kidanger/ipol-demorunner/blob/master/src/compilation.rs
#[macro_use]
extern crate rocket;
mod image_id;
mod user;

use crate::image_id::ImageId;

use std::fs;
use std::path::Path;
use sqlx::Row;
use chrono::Utc;
use rocket::Responder;
use rocket::serde::Deserialize;
use rocket::fairing::AdHoc;
use rocket_db_pools::sqlx;
use rocket_db_pools::Database;

#[derive(Debug, Responder)]
#[response(status = 500, content_type = "json")]
enum RoxideError {
    PermissionDenied(String),
    ExpiredImage(String),
    NotAnImage(String),
    ImageUnavailable(String),
    Database(String),
    IO(String),
}

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct AppConfig {
    upload_directory: String,
    id_length: usize,
}

fn is_token_valid(_token: &str) -> bool {
    true
}

#[derive(Database)]
#[database("sqlite_logs")]
struct Canard(sqlx::SqlitePool);


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
        .attach(AdHoc::on_liftoff("Database Cleanning", |rocket| {
            Box::pin(async move {
                let conn = match Canard::fetch(rocket) {
                    Some(pool) => pool.clone(), // clone the wrapped pool
                    None => panic!("Cannot fetch database"),
                };
                let now = Utc::now().timestamp();
                let expired_rows = sqlx::query("SELECT id FROM images WHERE expiration_date < $1")
                    .bind(&now)
                    .fetch_all(&**conn)
                    .await;
                if let Ok(expired_rows) = expired_rows {
                    for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
                        let id = ImageId::from(id);
                        let deleted = fs::remove_file(id.file_path("./upload"));
                        if let Err(err) = deleted {
                            eprintln!("Cannot delete {:?}", err);
                        }
                    }
                    sqlx::query("DELETE FROM images WHERE expiration_date < $1")
                        .bind(&now)
                        .execute(&**conn)
                        .await
                        .unwrap();
                }
            })
        }))
        .attach(user::stage())
        .launch()
        .await?;

    Ok(())
}
