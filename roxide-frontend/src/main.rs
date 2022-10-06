use wasm_bindgen::JsValue;
use web_sys::{Event, HtmlInputElement, UrlSearchParams};
use yew::{html, html::TargetCast, Component, Context, Html};

use gloo_file::File;
use gloo_net::http::Request;

pub enum Msg {
    Files(Vec<File>),
    Upload,
    Uploaded(String),
}

pub struct Model {
    files: Vec<File>,
    results: Vec<String>,
    duration: Option<i64>,
}

fn get_location_token() -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    UrlSearchParams::new_with_str(&search).ok()?.get("token")
}

fn get_url_of(upload_id: &str) -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();
    let protocol = location.protocol().ok()?;
    let host = location.host().ok()?;
    Some(format!("{protocol}//{host}/get/{upload_id}"))
}

async fn upload_file(file: File, token: &str, duration: Option<i64>) -> Result<Msg, JsValue> {
    let name = file.name();

    let form = web_sys::FormData::new()?;
    form.append_with_blob_and_filename("upload", file.as_ref(), &name)?;
    form.append_with_str("title", &name)?;
    if let Some(duration) = duration {
        form.append_with_str("duration", &duration.to_string())?;
    }

    let token = token.to_owned();
    let res = Request::post(&format!("/post/{}", token))
        .body(form)
        .send()
        .await;
    if let Err(err) = res {
        return Err(JsValue::from_str(&err.to_string()));
    }

    let res = res.unwrap().text().await;
    if let Err(err) = res {
        return Err(JsValue::from_str(&err.to_string()));
    }

    let res = res.unwrap();
    Ok(Msg::Uploaded(res))
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            files: vec![],
            results: vec![],
            duration: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Files(files) => {
                for file in files.into_iter() {
                    self.files.push(file);
                }
                true
            }
            Msg::Upload => {
                if let Some(token) = get_location_token() {
                    let duration = self.duration;
                    self.files.drain(..).for_each(|file| {
                        let token = token.to_owned();
                        ctx.link().send_future(async move {
                            match upload_file(file, &token, duration).await {
                                Ok(msg) => msg,
                                Err(_) => todo!(),
                            }
                        });
                    });
                } else {
                    let window = web_sys::window().unwrap();
                    window
                        .alert_with_message("Token not found. Use ?token=<your token> in the url.")
                        .unwrap();
                }
                true
            }
            Msg::Uploaded(res) => {
                if let Some(url) = get_url_of(&res) {
                    self.results.push(url);
                }
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_click = |e: Event| {
            let mut result = Vec::new();
            let input: HtmlInputElement = e.target_unchecked_into();

            if let Some(files) = input.files() {
                let files = js_sys::try_iter(&files)
                    .unwrap()
                    .unwrap()
                    .map(|v| web_sys::File::from(v.unwrap()))
                    .map(File::from);
                result.extend(files);
            }
            Msg::Files(result)
        };

        html! {
            <div>
                <div>
                    <p>{ "Choose files to upload" }</p>
                    <input type="file" multiple=true onchange={ ctx.link().callback(on_click) }
                    />
                </div>
                <ul>
                    { for self.files.iter().map(Self::view_file) }
                </ul>
                <div>
                    <input value="Upload" type="button" onclick={ctx.link().callback(|_| Msg::Upload)} />
                </div>
                <div>
                    { for self.results.iter().map(|url| Self::view_url(url.to_string())) }
                </div>
            </div>
        }
    }
}

impl Model {
    fn view_file(data: &File) -> Html {
        let name = data.name();
        let mimetype = data.raw_mime_type();
        let size = data.size();
        html! {
            <li>{ format!("{}: {}, {}kb", name, mimetype, size / 1024) }</li>
        }
    }

    fn view_url(url: String) -> Html {
        let url2 = url.clone();
        html! {
            <p><a href={url}> {url2} </a></p>
        }
    }
}

fn main() {
    yew::start_app::<Model>();
}
