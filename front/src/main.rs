use web_sys::{Event, HtmlInputElement};
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
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            files: vec![],
            results: vec![],
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
                let token = "TODO";
                for file in self.files.iter() {
                    let form = web_sys::FormData::new().unwrap();
                    let name = &file.name();
                    form.append_with_blob_and_filename("upload", file.as_ref(), name)
                        .unwrap();
                    form.append_with_str("title", name).unwrap();
                    ctx.link().send_future(async move {
                        let res = Request::post(&format!("/user/post/{}", token))
                            .body(form)
                            .send()
                            .await
                            .unwrap()
                            .text()
                            .await
                            .unwrap();
                        Msg::Uploaded(res)
                    });
                }
                true
            }
            Msg::Uploaded(res) => {
                self.results.push(res);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div>
                <div>
                    <p>{ "Choose files to upload" }</p>
                    <input type="file" multiple=true onchange={ctx.link().callback(move |e: Event| {
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
                        })}
                    />
                </div>
                <ul>
                    { for self.files.iter().map(Self::view_file) }
                </ul>
                <div>
                    <input value="Upload" type="button" onclick={ctx.link().callback(|_| Msg::Upload)} />
                </div>
                <div>
                    { format!("{:?}", self.results) }
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
            <li>{ format!("{}: {}, {}", name, mimetype, size) }</li>
        }
    }
}

fn main() {
    yew::start_app::<Model>();
}
