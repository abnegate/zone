use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub license: &'a str,
}
