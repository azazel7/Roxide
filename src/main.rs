//https://github.com/kidanger/ipol-demorunner/blob/master/src/compilation.rs
#[macro_use]
extern crate rocket;
use rocket::fairing::AdHoc;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::request::FromParam;
use rocket::tokio::fs::File;
use rocket::Config;
use std::fs;

use rand::{self, Rng};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rocket::tokio;
use std::time::Duration;

use rocket_db_pools::sqlx;
use rocket_db_pools::{Connection, Database};
use sqlx::Row;

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

fn is_token_valid(token: &str) -> bool {
    true
}
#[get("/get/<token>/<id>")]
async fn get(mut db: Connection<Canard>, token: &str, id: ImageId) -> Option<File> {
    if !is_token_valid(token) {
        return None;
    }
    let now = Utc::now().timestamp();
    let db_entry = sqlx::query("SELECT expiration_date FROM images WHERE id = $1")
        .bind(&id.get_id())
        .fetch_one(&mut *db)
        .await;
    if let Ok(row) = db_entry {
        let expiration_date = row.get::<i64, &str>("expiration_date");
        if expiration_date <= now {
            return None;
        }
    } else {
        return None;
    }
    File::open(id.file_path()).await.ok()
}
async fn clean_expired_images(db: &Canard) {
    let now = Utc::now().timestamp();
    dbg!(now);
    let expired_rows = sqlx::query("SELECT id FROM images WHERE expiration_date < $1")
        .bind(&now)
        .fetch_all(&**db)
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
            .execute(&**db)
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
    mut db: Connection<Canard>,
    token: &str,
    mut img: Form<Upload<'_>>,
) -> std::io::Result<String> {
    let id = ImageId::new(ID_LENGTH);
    if !is_token_valid(token) {
        return Err(Error::new(ErrorKind::PermissionDenied, "Token not valid"));
    }

    let expiration = img.duration.map_or(i64::MAX - 1, |duration| {
        let now = Utc::now().timestamp();
        now + duration
    });

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

        Ok(id.get_id())
    }
}

#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    let url: String = Config::figment()
        .extract_inner("databases.sqlite_logs.url")
        .unwrap();
	tokio::spawn(async move {
	  let mut ten_minutes = time::interval(Duration::from_secs(60 * 10));
	  loop {
		eprintln!("{:}", url);

		ten_minutes.tick().await;
	  }
	});

    let _r = rocket::build()
        .attach(Canard::init())
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
        //.attach(AdHoc::on_liftoff("DB polling", |rocket| {
            //Box::pin(async move {
                //let conn = Canard::fetch(&rocket);
                //rocket::tokio::spawn(async move {
                    //let mut interval = rocket::tokio::time::interval(
                        //rocket::tokio::time::Duration::from_secs(10),
                        //);
                    //loop {
                        //interval.tick().await;
                        //// do_sql_stuff(&conn).await;
                        //clean_expired_images(conn.unwrap());
                        //println!("Do something here!!!");
                    //}
                //});
            //})
        //}))
        .mount("/", routes![get])
        .mount("/", routes![post])
        .launch()
        .await?;

    Ok(())
}
//.attach(AdHoc::try_on_ignite("Background job", |rocket| async {
//let conn = match Canard::fetch(&rocket) {
//Some(pool) => pool.clone(), // clone the wrapped pool
//None => return Err(rocket),
//};

//rocket::tokio::task::spawn(async move {
//loop {
//eprintln!("Cleaning");
//clean_expired_images(conn);
//tokio::time::sleep(Duration::from_secs(10)).await;
//}
//});
//Ok(rocket)
//}))
