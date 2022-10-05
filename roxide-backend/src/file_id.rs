use rand::seq::SliceRandom;
use rocket::request::FromParam;
use std::path::{Path, PathBuf};

///The structure to manage ids of images.
#[derive(Debug)]
pub struct FileId {
    id: String,
}

impl FileId {
    ///Build a new FileId with an id size based on the *size* parameter.
    pub fn new(size: usize) -> Self {
        //All the symbols to use
        const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

        let mut rng = rand::thread_rng();

        let id = (0..size)
            .into_iter()
            .map(|_| *BASE62.choose(&mut rng).unwrap())
            .collect::<Vec<_>>();

        let id = std::str::from_utf8(&id).unwrap().to_string();

        Self { id }
    }

    ///Build a new FileId from an existing id.
    pub fn from(id: &str) -> Self {
        Self { id: id.to_string() }
    }

    ///Compute the path of the file.
    pub fn file_path(&self, root: &str) -> PathBuf {
        Path::new(root).join(&self.id)
    }

    ///Return a reference to the id.
    pub fn get_id(&self) -> &str {
        &self.id
    }
}

///Allows to build an ImageId from a String, assuming this string is alphanumeric.
impl<'a> FromParam<'a> for FileId {
    type Error = &'a str;

    fn from_param(param: &'a str) -> Result<Self, Self::Error> {
        param
            .chars()
            .all(|c| c.is_ascii_alphanumeric())
            .then(|| Self { id: param.into() })
            .ok_or("token read from parameter is invalid")
    }
}
