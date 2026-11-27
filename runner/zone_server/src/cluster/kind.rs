use std::time::Duration;

const REQUEST_LIFETIME: Duration = Duration::from_secs(10);
const LIFETIME: Duration = Duration::from_secs(30);

/// What an envelope is for, which decides how the bus routes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Message = 1,
    Request = 2,
    Reply = 3,
}

impl Kind {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Message),
            2 => Some(Self::Request),
            3 => Some(Self::Reply),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// How long after it was issued an envelope is still delivered. A request
    /// acts on a chat or a task, so one captured off the wire stays usable
    /// for the shortest time.
    pub fn lifetime(self) -> Duration {
        match self {
            Self::Request => REQUEST_LIFETIME,
            Self::Message | Self::Reply => LIFETIME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_byte() {
        for kind in [Kind::Message, Kind::Request, Kind::Reply] {
            assert_eq!(Kind::from_byte(kind.as_byte()), Some(kind));
        }
        assert_eq!(Kind::from_byte(0), None);
        assert_eq!(Kind::from_byte(4), None);
    }

    #[test]
    fn a_request_lives_shorter_than_anything_else() {
        assert!(Kind::Request.lifetime() < Kind::Message.lifetime());
        assert_eq!(Kind::Message.lifetime(), Kind::Reply.lifetime());
    }
}
