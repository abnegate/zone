use aws_lc_rs::digest::Context;

pub(super) trait Sink {
    fn push_str(&mut self, value: &str);
}

impl Sink for String {
    fn push_str(&mut self, value: &str) {
        String::push_str(self, value);
    }
}

impl Sink for Vec<u8> {
    fn push_str(&mut self, value: &str) {
        self.extend_from_slice(value.as_bytes());
    }
}

impl Sink for Context {
    fn push_str(&mut self, value: &str) {
        self.update(value.as_bytes());
    }
}
