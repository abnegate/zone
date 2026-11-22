use std::cell::RefCell;

use tracing::{Level, Metadata};

thread_local! {
    static CURRENT: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

pub(super) struct Capture {
    level: Level,
    written: Vec<u8>,
}

impl Capture {
    pub(super) fn begin(level: Level) {
        CURRENT.set(Some(Self {
            level,
            written: Vec::new(),
        }));
    }

    pub(super) fn end() -> String {
        let capture = CURRENT.take().expect("a capture was begun on this thread");
        String::from_utf8(capture.written).unwrap()
    }

    pub(super) fn admits(metadata: &Metadata<'_>) -> bool {
        CURRENT.with_borrow(|current| {
            current
                .as_ref()
                .is_some_and(|capture| *metadata.level() <= capture.level)
        })
    }

    pub(super) fn append(bytes: &[u8]) {
        CURRENT.with_borrow_mut(|current| {
            if let Some(capture) = current {
                capture.written.extend_from_slice(bytes);
            }
        });
    }
}
