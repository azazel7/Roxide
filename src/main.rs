#[macro_use]
extern crate rocket;
use rocket::form::Form;
use rocket::fs::TempFile;
use rocket::request::FromParam;
use rocket::tokio::fs::File;

use rand::{self, Rng};
use rusqlite::{Connection, Result};
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

const ID_LENGTH: usize = 3;
const DATABASE_PATH: &str = "./my_db.db3";
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
#[derive(Debug, FromForm)]
struct Upload<'f> {
    upload: TempFile<'f>,
    duration: Option<u64>,
}

fn get_connection() -> Connection {
    let path = DATABASE_PATH;
    Connection::open(path).unwrap()
}
#[post("/post/<token>", data = "<img>")]
async fn post(token: &str, mut img: Form<Upload<'_>>) -> std::io::Result<String> {
    let id = ImageId::new(ID_LENGTH);
    if !is_token_valide(token) {
        Err(Error::new(ErrorKind::Other, "oh no!"))
    } else {
        img.upload.copy_to(id.file_path()).await?;
        let db = get_connection();
        let expiration = &"2001".to_string();
        let a = db.execute(
            "INSERT INTO images (id, expiration_date, token_used) VALUES (:id, :expiration, :token)",
            &[(":id", &id.get_id()), (":token", &token.to_string()), (":expiration", expiration) ],
        );
        eprintln!("{:?}", a);

        Ok("All good buddy".to_string())
    }
}

#[launch]
fn rocket() -> _ {
    let init = !Path::new(DATABASE_PATH).exists();
    let db = get_connection();
    if init {
        eprintln!("Initializing data base");
        db.execute(
            "CREATE TABLE images (
            id    TEXT PRIMARY KEY,
            expiration_date  DATE,
            token_used TEXT
        )",
            (), // empty list of parameters.
        )
        .unwrap();
    }
    let mut stmt = db.prepare("SELECT id, expiration_date, token_used FROM images").unwrap();
    let person_iter = stmt.query_map([], |row| {
        Ok((row.get(0), row.get(1), row.get(2)))
    });
    db.close().unwrap();

    rocket::build()
        .mount("/", routes![get])
        .mount("/", routes![post])
}
