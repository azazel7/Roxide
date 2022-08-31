use std::fs;
use std::path::Path;

use chrono::Utc;

use rocket::fairing::AdHoc;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::http::ContentType;
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::tokio::fs::File;
use rocket::State;

use rocket_db_pools::Connection;

use sqlx::Row;

use crate::{is_token_valid, AppConfig, Canard, FileId, RoxideError};

//Structure use to receive the form that post a file.
#[derive(Debug, FromForm)]
struct UploadFile<'f> {
    upload: TempFile<'f>,
    title: String,
    duration: Option<i64>,
    unlisted: Option<bool>,
}

/// Function that process a new posted file.
///
/// This function checks the following:
/// - the token is valid.
/// - the duration is correct.
///
#[post("/post/<token>", data = "<upload_form>")]
async fn post(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    token: &str,
    mut upload_form: Form<UploadFile<'_>>,
) -> Result<String, RoxideError> {
    if !is_token_valid(token, app_config) {
        return Err(RoxideError::Roxide("Token not valid".to_string()));
    }
    let mut id = FileId::new(app_config.id_length);
    while Path::new(&id.file_path(&app_config.upload_directory)).exists() {
        id = FileId::new(app_config.id_length);
    }

    let now = Utc::now().timestamp();
    let expiration = upload_form
        .duration
        .map_or(i64::MAX - 1, |duration| now + duration);
    if expiration < now {
        return Err(RoxideError::Roxide("Expired file".to_string()));
    }
    let size;
    let content_type;
    if let Some(file_path) = upload_form.upload.path() {
        let kind = infer::get_from_path(file_path).expect("file read successfully");

        content_type = kind.map_or("unknown", |s| s.mime_type());
        let metadata = fs::metadata(file_path)?;
        size = metadata.len() as i64;
    } else {
        return Err(RoxideError::Roxide("No path to the file".to_string()));
    }

    //Retrieve the database entry
    let time_limit = now - 3600;
    let db_count = sqlx::query(
        "SELECT count(1) AS count FROM files WHERE token_used = $1 AND upload_date > $2",
    )
    .bind(token)
    .bind(&time_limit)
    .fetch_one(&mut *db)
    .await?;

    let count = db_count.get::<i64, &str>("count") as usize;
    if count >= app_config.max_upload {
        return Err(RoxideError::Roxide("Too much upload".to_string()));
    }

    // Set if the the file is public from the unlisted parameter
    let public = !upload_form.unlisted.unwrap_or(false);

    // Insert the new entry to the database
    sqlx::query(
        "INSERT INTO files (id, expiration_date, upload_date, token_used, content_type, size, download_count, public, title) VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8) RETURNING *",
    )
    .bind(id.get_id())
    .bind(&expiration)
    .bind(&now)
    .bind(&token)
    .bind(content_type)
    .bind(&size)
    .bind(&public)
    .bind(&upload_form.title)
    .execute(&mut *db)
    .await?;

    //If everything went nicely, copy the file
    let copy = upload_form
        .upload
        .copy_to(id.file_path(&app_config.upload_directory))
        .await;

    // Failed to copy, so we delete entry from database then we propagate the error
    if let Err(copy) = copy {
        sqlx::query("DELETE files WHERE id = $1")
            .bind(id.get_id())
            .execute(&mut *db)
            .await?;

        return Err(RoxideError::IO(copy));
    }
    Ok(id.get_id().to_string())
}

/// Function that retrieve and return a file based on its id.
///
/// An error is return if the id doesn't exist or if the file has expired. In the case of an
/// expired file, the function triggers a cleanning of the database.
#[get("/get/<id>")]
async fn get(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    id: FileId,
) -> Result<(ContentType, File), RoxideError> {
    //Retrieve the database entry
    let row = sqlx::query("SELECT expiration_date, content_type FROM files WHERE id = $1")
        .bind(id.get_id())
        .fetch_one(&mut *db)
        .await?;
    let expiration_date = row.get::<i64, &str>("expiration_date");
    let now = Utc::now().timestamp();

    //Check expiration date and clean the database if expired
    if expiration_date <= now {
        // FIXME ok, this a copy-paste of clean_expired_files as Rust and Rocket won't allow to
        // passe connections through async function.

        //Select all expired files
        let expired_rows = sqlx::query("SELECT id FROM files WHERE expiration_date < $1")
            .bind(&now)
            .fetch_all(&mut **db)
            .await?;

        //Iterate over the row to delete the files
        for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
            let id = FileId::from(id);
            fs::remove_file(id.file_path(&app_config.upload_directory))?;
        }

        //Delete the expired files from the database
        sqlx::query("DELETE FROM files WHERE expiration_date < $1")
            .bind(&now)
            .execute(&mut **db)
            .await?;
    }

    let content_type = ContentType::parse_flexible(row.get::<&str, &str>("content_type"))
        .unwrap_or(ContentType::Any);

    //Delete the expired files from the database
    sqlx::query("UPDATE files SET download_count = download_count+1 WHERE id = $1")
        .bind(id.get_id())
        .execute(&mut **db)
        .await?;

    //ContentType is set to PNG as there is no ContentType that designates a file in general.
    Ok((
        content_type,
        File::open(id.file_path(&app_config.upload_directory)).await?,
    ))
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct FileData {
    id: String,
    upload_date: i64,
    content_type: String,
    download_count: i64,
    size: i64,
    title: String,
}

type ListFiles = Vec<FileData>;

#[get("/list/<token>")]
async fn list(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
    token: &str,
) -> Result<Json<ListFiles>, RoxideError> {
    if !is_token_valid(token, app_config) {
        return Err(RoxideError::Roxide("Token not valid".to_string()));
    }
    //Retrieve the database entry
    let now = Utc::now().timestamp();
    let public_files = sqlx::query("SELECT id, upload_date, content_type, download_count, size, title FROM files WHERE public = true AND expiration_date > $1")
        .bind(&now)
        .fetch_all(&mut *db)
        .await?;

    let it = public_files
        .iter()
        .map(|row| FileData {
            id: row.get::<String, &str>("id"),
            upload_date: row.get::<i64, &str>("upload_date"),
            content_type: row.get::<String, &str>("content_type"),
            download_count: row.get::<i64, &str>("download_count"),
            size: row.get::<i64, &str>("size"),
            title: row.get::<String, &str>("title"),
        })
        .collect::<ListFiles>();

    Ok(Json(it))
}

/// Function that clean the database from expired files.
#[get("/clean")]
async fn clean(app_config: &State<AppConfig>, db: Connection<Canard>) -> Option<File> {
    clean_expired_files(app_config, db).await.unwrap();
    None
}

/// Function that clean the database from expired files (it is called by clean and get).
async fn clean_expired_files(
    app_config: &State<AppConfig>,
    mut db: Connection<Canard>,
) -> Result<(), RoxideError> {
    let now = Utc::now().timestamp();

    //Select all expired files
    let expired_rows = sqlx::query("SELECT id FROM files WHERE expiration_date < $1")
        .bind(&now)
        .fetch_all(&mut **db)
        .await?;

    //Iterate over the row to delete the files
    for id in expired_rows.iter().map(|row| row.get::<&str, &str>("id")) {
        let id = FileId::from(id);
        fs::remove_file(id.file_path(&app_config.upload_directory))?;
    }

    //Delete the expired files from the database
    sqlx::query("DELETE FROM files WHERE expiration_date < $1")
        .bind(&now)
        .execute(&mut **db)
        .await?;

    Ok(())
}

/// Function that mounts the routes for user URL in Rocket.
/// - get (to retrieve a file).
/// - post (to upload a file).
/// - clean (to trigger a cleanning of the database)
pub fn stage() -> AdHoc {
    AdHoc::on_ignite("User stage", |rocket| async {
        rocket.mount("/user", routes![get, post, clean, list])
    })
}
