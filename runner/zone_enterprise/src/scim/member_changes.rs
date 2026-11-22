use std::collections::HashSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberChanges {
    pub replace: Option<Vec<String>>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub clear: bool,
}

impl MemberChanges {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn replace_with(&mut self, members: Vec<String>) {
        *self = Self {
            replace: Some(appended(Vec::new(), members)),
            ..Self::default()
        };
    }

    pub(crate) fn clear_all(&mut self) {
        *self = Self {
            clear: true,
            ..Self::default()
        };
    }

    pub(crate) fn add_all(&mut self, members: Vec<String>) {
        if let Some(replace) = self.replace.take() {
            self.replace = Some(appended(replace, members));
            return;
        }
        let added: HashSet<&String> = members.iter().collect();
        self.remove.retain(|member| !added.contains(member));
        self.add = appended(std::mem::take(&mut self.add), members);
    }

    pub(crate) fn remove_all(&mut self, members: Vec<String>) {
        let removed: HashSet<&String> = members.iter().collect();
        if let Some(replace) = self.replace.as_mut() {
            replace.retain(|member| !removed.contains(member));
            return;
        }
        self.add.retain(|member| !removed.contains(member));
        if !self.clear {
            self.remove = appended(std::mem::take(&mut self.remove), members);
        }
    }
}

fn appended(mut list: Vec<String>, members: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = list.iter().cloned().collect();
    for member in members {
        if seen.insert(member.clone()) {
            list.push(member);
        }
    }
    list
}
