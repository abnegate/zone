use super::Entry;

#[derive(Debug)]
pub struct Section {
    pub title: &'static str,
    pub entries: &'static [Entry],
}
