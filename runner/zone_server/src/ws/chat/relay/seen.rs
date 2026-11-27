use std::collections::{HashSet, VecDeque};

use uuid::Uuid;

/// The ids of the messages a replica took from other replicas, so one resent
/// after its acknowledgement went missing is answered as a duplicate instead
/// of run again. A resend follows its first try within seconds, so only the
/// newest `capacity` are kept.
#[derive(Debug)]
pub struct Seen {
    order: VecDeque<Uuid>,
    ids: HashSet<Uuid>,
    capacity: usize,
}

impl Seen {
    pub fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::with_capacity(capacity),
            ids: HashSet::with_capacity(capacity),
            capacity,
        }
    }

    pub fn contains(&self, id: &Uuid) -> bool {
        self.ids.contains(id)
    }

    pub fn insert(&mut self, id: Uuid) {
        if !self.ids.insert(id) {
            return;
        }
        self.order.push_back(id);
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.ids.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_id_is_forgotten_once_the_capacity_is_reached() {
        let mut seen = Seen::new(2);
        let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];

        for id in ids {
            seen.insert(id);
        }

        assert_eq!(seen.order.len(), 2);
        assert_eq!(seen.ids.len(), 2);
        assert!(!seen.contains(&ids[0]));
        assert!(seen.contains(&ids[1]));
        assert!(seen.contains(&ids[2]));
    }

    #[test]
    fn an_id_seen_again_takes_no_second_place() {
        let mut seen = Seen::new(2);
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());

        seen.insert(first);
        seen.insert(first);
        seen.insert(second);

        assert_eq!(seen.order.len(), 2);
        assert!(seen.contains(&first));
        assert!(seen.contains(&second));
    }
}
