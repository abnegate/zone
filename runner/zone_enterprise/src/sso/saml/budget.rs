use crate::sso::saml::sink::Sink;

pub(super) struct Budget<'a, S: Sink> {
    sink: &'a mut S,
    remaining: usize,
    exhausted: bool,
}

impl<'a, S: Sink> Budget<'a, S> {
    pub(super) fn new(sink: &'a mut S, limit: usize) -> Self {
        Self {
            sink,
            remaining: limit,
            exhausted: false,
        }
    }

    pub(super) fn exhausted(&self) -> bool {
        self.exhausted
    }
}

impl<S: Sink> Sink for Budget<'_, S> {
    fn push_str(&mut self, value: &str) {
        if self.exhausted {
            return;
        }
        match self.remaining.checked_sub(value.len()) {
            Some(remaining) => {
                self.remaining = remaining;
                self.sink.push_str(value);
            }
            None => self.exhausted = true,
        }
    }
}
