# Roxide

Roxide is backend web app to upload, store, and share files. The submitted file can have an expiration date after which it is deleted.

## Install

Using cargo and git:

```sh
git clone https://github.com/azazel7/Roxide.git 
cd Roxide
cargo build
```

Using nix:
```sh
nix run github:azazel7/Roxide
```

## Configure

Use the file `Rocket.toml` to configure Roxide.

- `url` Indicate the path to the sqlite database. Both fiel must be equal.
- `upload_directory` Indicate the directory where files will be stored.
- `id_length` is the size of the id used for the files. The higher, the less collision between file ids.
- `limits` is a field used by Rocket to define the maximum size that can be submitted. See [here](https://api.rocket.rs/v0.5-rc/rocket/data/struct.Limits.html#built-in-limits) and [here](https://rocket.rs/v0.5-rc/guide/configuration/#limits) for more information.
- `max_upload` Indicates the maximum upload a token can do per hour.
- `cleaning_frequency` is the time in second between two periodic cleaning of the database.
- `check_token` indicates if we should check the token with redis.
- `front_sources` indicates the path to the front sources (index.html and other files)

## Run

```sh
cd roxide-frontend && trunk build && cd ..
cargo run --bin roxide-backend
```
