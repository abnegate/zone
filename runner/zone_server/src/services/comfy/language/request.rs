use abnegate_comfy::lora::TrainImage;
use abnegate_comfy::lora::TrainMethod;

#[derive(Debug)]
pub struct Request {
    pub name: String,
    pub base: String,
    pub method: TrainMethod,
    pub documents: Vec<TrainImage>,
}

impl Request {
    pub fn new(
        name: impl Into<String>,
        base: impl Into<String>,
        documents: Vec<TrainImage>,
    ) -> Self {
        Self {
            name: name.into(),
            base: base.into(),
            method: TrainMethod::default(),
            documents,
        }
    }

    pub fn with_method(mut self, method: TrainMethod) -> Self {
        self.method = method;
        self
    }
}
