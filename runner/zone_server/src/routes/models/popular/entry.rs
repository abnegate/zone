/// One of the models Browse lists when ollama.com's search page lists none.
pub(super) struct Entry {
    pub(super) name: &'static str,
    pub(super) family: &'static str,
    pub(super) parameter_size: &'static str,
    pub(super) description: &'static str,
    pub(super) use_cases: &'static [&'static str],
}
