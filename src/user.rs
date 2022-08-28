use std::fs;
use std::path::Path;

use chrono::Utc;

use image::io::Reader as ImageReader;

use rocket::fairing::AdHoc;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::http::ContentType;
use rocket::tokio::fs::File;
use rocket::State;

use rocket_db_pools::Connection;

use sqlx::Row;

use crate::{is_token_valid, AppConfig, Canard, ImageId, RoxideError};

//Structure use to receive the form that post an image.
#[derive(Debug, FromForm)]
struct Upload<'f> {
    upload: TempFile<'f>,
    duration: Option<i64>,
}

/// Function that process a new posted image.
///
/// This function checks the following:
/// - the token is valid.
/// - the duration is correct.
/// - the file is an image.
///
#[post("/post/<token>", data = "<img>")]
async fn post(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    token: &str,
    mut img: Form<Upload<'_>>,
) -> Result<String, RoxideError> {
    if !is_token_valid(token) {
        return Err(RoxideError::Roxide("Token not valid".to_string()));
    }
    let mut id = ImageId::new(app_config.id_length);
    while Path::new(&id.file_path(&app_config.upload_directory)).exists() {
        id = ImageId::new(app_config.id_length);
    }

    let now = Utc::now().timestamp();
    let expiration = img.duration.map_or(i64::MAX - 1, |duration| now + duration);
    if expiration < now {
        return Err(RoxideError::Roxide("Expired image".to_string()));
    }
    if let Some(image_path) = img.upload.path() {
        let img = ImageReader::open(image_path)?;
        let dec = img.with_guessed_format()?;
        let dec = dec.decode();
        if dec.is_err() {
            return Err(RoxideError::Roxide("Not an image".to_string()));
        }
    } else {
        return Err(RoxideError::Roxide("No path to the image".to_string()));
    }

    //Retrieve the database entry
    let time_limit = now - 3600;
    let db_count = sqlx::query(
        "SELECT count(1) AS count FROM images WHERE token_used = $1 AND upload_date > $2",
    )
    .bind(token)
    .bind(&time_limit)
    .fetch_one(&mut *db)
    .await?;
    let count = db_count.get::<i64, &str>("count") as usize;
    if count >= app_config.max_upload {
        return Err(RoxideError::Roxide("Too much upload".to_string()));
    }

    sqlx::query(
        "INSERT INTO images (id, expiration_date, upload_date, token_used, is_image) VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(id.get_id())
    .bind(&expiration)
    .bind(&now)
    .bind(&token)
    .bind(&true)
    .execute(&mut *db)
    .await?;
    img.upload
        .copy_to(id.file_path(&app_config.upload_directory))
        .await?;
    Ok(id.get_id().to_string())
}

/// Function that retrieve and return an image based on its id.
///
/// An error is return if the id doesn't exist or if the image has expired. In the case of an
/// expired image, the function triggers a cleanning of the database.
#[get("/get/<id>")]
async fn get(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    id: ImageId,
) -> (ContentType, Option<File>) {
    //Retrieve the database entry
    let db_entry = sqlx::query("SELECT expiration_date FROM images WHERE id = $1")
        .bind(id.get_id())
        .fetch_one(&mut *db)
        .await;
    if let Ok(row) = db_entry {
        let expiration_date = row.get::<i64, &str>("expiration_date");
        let now = Utc::now().timestamp();
        //Check expiration date and clean the database if expired
        if expiration_date <= now {
            if clean_expired_images(app_config, db).await.is_err() {
                eprintln!("Couldn't clean database");
            }
            return (ContentType::Any, None);
        }
    } else {
        //The entry in the database cannot be found.
        return (ContentType::Any, None);
    }

    //ContentType is set to PNG as there is no ContentType that designates an image in general.
    (
        ContentType::PNG,
        File::open(id.file_path(&app_config.upload_directory))
            .await
            .ok(),
    )
}

/// Function that clean the database from expired images.
#[get("/clean")]
async fn clean(app_config: &State<AppConfig>, db: Connection<Canard>) -> Option<File> {
    clean_expired_images(app_config, db).await;
    None
}

/// Function that clean the database from expired images (it is called by clean and get).
async fn clean_expired_images(app_config: &State<AppConfig>, mut db: Connection<Canard>) -> Result<(), RoxideError>{
    let now = Utc::now().timestamp();

    //Select all expired images
    let expired_rows = sqlx::query("SELECT id FROM images WHERE expiration_date < $1")
        .bind(&now)
        .fetch_all(&mut **db)
        .await?;

    //Iterate over the row to delete the files
    for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
        let id = ImageId::from(id);
        fs::remove_file(id.file_path(&app_config.upload_directory))?;
    }

    //Delete the expired images from the database
    sqlx::query("DELETE FROM images WHERE expiration_date < $1")
        .bind(&now)
        .execute(&mut **db)
        .await?;

    Ok(())
}

/// Function that mounts the routes for user URL in Rocket.
/// - get (to retrieve an image).
/// - post (to upload an image).
/// - clean (to trigger a cleanning of the database)
pub fn stage() -> AdHoc {
    AdHoc::on_ignite("User stage", |rocket| async {
        rocket.mount("/user", routes![get, post, clean])
    })
}
