use std::collections::{HashMap, HashSet};
use std::ptr;
use std::sync::Arc;

use crate::sso::saml::name::Name;

type Identity = (Option<*const str>, *const str, Option<*const str>);

#[derive(Default)]
pub(super) struct Interner {
    strings: HashSet<Arc<str>>,
    names: HashMap<Identity, Arc<Name>>,
}

impl Interner {
    pub(super) fn intern(&mut self, value: &str) -> Arc<str> {
        if let Some(existing) = self.strings.get(value) {
            return Arc::clone(existing);
        }
        let interned: Arc<str> = Arc::from(value);
        self.strings.insert(Arc::clone(&interned));
        interned
    }

    pub(super) fn name(
        &mut self,
        prefix: Option<&str>,
        local: &str,
        namespace: Option<Arc<str>>,
    ) -> Arc<Name> {
        let prefix = prefix.map(|prefix| self.intern(prefix));
        let local = self.intern(local);
        let key = (
            prefix.as_deref().map(ptr::from_ref),
            ptr::from_ref(&*local),
            namespace.as_deref().map(ptr::from_ref),
        );
        let name = self.names.entry(key).or_insert_with(|| {
            Arc::new(Name {
                prefix,
                local,
                namespace,
            })
        });
        Arc::clone(name)
    }
}
