use sqlx::PgPool;
use uuid::Uuid;

use super::super::ServerMessage;
use super::holding::Holding;
use super::persisted::Persisted;

/// A turn another replica runs that this one shows and has not seen end.
/// `fence` is the lease the turn runs under, once it has been read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Watch {
    open: bool,
    fence: Option<i64>,
}

impl Watch {
    pub fn observe(&mut self, terminator: bool) {
        if terminator {
            *self = Self::default();
        } else {
            self.open = true;
        }
    }

    pub fn arm(&mut self, fence: i64) {
        self.open = true;
        self.fence = Some(fence);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Reads the lease of the turn being watched. Once the lease the turn ran
    /// under has expired or passed on, the turn is over whether or not its
    /// last frame arrived, so this returns the frame that ends it, read from
    /// what was saved.
    pub async fn check(&mut self, pool: &PgPool, chat: Uuid) -> Option<ServerMessage> {
        if !self.open {
            return None;
        }
        let holding = match Holding::read(pool, chat).await {
            Ok(holding) => holding,
            Err(error) => {
                tracing::warn!(%chat, %error, "Could not read the lease of a relayed turn");
                return None;
            }
        };
        let Some(fence) = self.fence else {
            match holding {
                Some(Holding { fence, live: true }) => self.fence = Some(fence),
                _ => self.clear(),
            }
            return None;
        };
        if holding == Some(Holding { fence, live: true }) {
            return None;
        }
        self.clear();
        match Persisted::at(pool, chat, fence).await {
            Ok(turn) => Some(Persisted::closing(turn)),
            Err(error) => {
                tracing::warn!(%chat, %error, "Could not read how a relayed turn ended");
                Some(Persisted::closing(None))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_opens_the_watch_and_a_terminator_closes_it() {
        let mut watch = Watch::default();

        watch.observe(false);
        assert!(watch.is_open());
        assert_eq!(watch.fence, None);

        watch.arm(4);
        watch.observe(false);
        assert_eq!(watch.fence, Some(4));

        watch.observe(true);
        assert_eq!(watch, Watch::default());
    }
}
