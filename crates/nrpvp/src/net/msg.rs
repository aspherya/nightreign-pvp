//! NET-1: the wire format of the mod-to-mod data channel (NETWORKING.md §4).
//!
//! One message = `[version u8][type u8][body]`, at most [`MAX_LEN`] bytes, little endian. The decoder is strict (exact body length,
//! known version and type) because every byte comes from another PC: it never indexes without a length check and never allocates
//! from a length field.

/// Wire version. Bump it when a message changes; builds with another version ignore each other's messages.
pub(crate) const VERSION: u8 = 1;
/// Hard cap on one message (the decoder refuses longer input before looking at it).
pub(crate) const MAX_LEN: usize = 256;

/// What mods say to each other. Facts only: no message asks the receiver to run or write anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Msg {
    /// Sent about once a second to every roster peer; opens the Steam session and shows the build.
    Hello {
        build: u16,
    },
    Ping {
        seq: u32,
    },
    Pong {
        seq: u32,
    },
    /// A fact for [`super::facts`]: "under `key` of `topic` the value is `value`". Features choose topic, key and value; the channel
    /// does not look inside. Resent until the receiver answers with [`Msg::Ack`].
    Fact {
        topic: u16,
        key: u32,
        value: u32,
    },
    /// "I have recorded the fact `(topic, key)`" (also sent for a duplicate, so the sender stops).
    Ack {
        topic: u16,
        key: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeError {
    TooShort,
    TooLong,
    Version(u8),
    Type(u8),
    Length { want: usize, got: usize },
}

const T_HELLO: u8 = 1;
const T_PING: u8 = 2;
const T_PONG: u8 = 3;
const T_FACT: u8 = 4;
const T_ACK: u8 = 5;

pub(crate) fn encode(m: &Msg) -> Vec<u8> {
    let mut v = vec![VERSION];
    match *m {
        Msg::Hello { build } => {
            v.push(T_HELLO);
            v.extend_from_slice(&build.to_le_bytes());
        }
        Msg::Ping { seq } => {
            v.push(T_PING);
            v.extend_from_slice(&seq.to_le_bytes());
        }
        Msg::Pong { seq } => {
            v.push(T_PONG);
            v.extend_from_slice(&seq.to_le_bytes());
        }
        Msg::Fact { topic, key, value } => {
            v.push(T_FACT);
            v.extend_from_slice(&topic.to_le_bytes());
            v.extend_from_slice(&key.to_le_bytes());
            v.extend_from_slice(&value.to_le_bytes());
        }
        Msg::Ack { topic, key } => {
            v.push(T_ACK);
            v.extend_from_slice(&topic.to_le_bytes());
            v.extend_from_slice(&key.to_le_bytes());
        }
    }
    v
}

pub(crate) fn decode(b: &[u8]) -> Result<Msg, DecodeError> {
    if b.len() > MAX_LEN {
        return Err(DecodeError::TooLong);
    }
    let [ver, ty, body @ ..] = b else {
        return Err(DecodeError::TooShort);
    };
    if *ver != VERSION {
        return Err(DecodeError::Version(*ver));
    }
    let want = |n: usize| -> Result<(), DecodeError> {
        if body.len() == n {
            Ok(())
        } else {
            Err(DecodeError::Length {
                want: n,
                got: body.len(),
            })
        }
    };
    let u32le = || u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    match *ty {
        T_HELLO => {
            want(2)?;
            Ok(Msg::Hello {
                build: u16::from_le_bytes([body[0], body[1]]),
            })
        }
        T_PING => {
            want(4)?;
            Ok(Msg::Ping { seq: u32le() })
        }
        T_PONG => {
            want(4)?;
            Ok(Msg::Pong { seq: u32le() })
        }
        T_FACT => {
            want(10)?;
            Ok(Msg::Fact {
                topic: u16::from_le_bytes([body[0], body[1]]),
                key: u32::from_le_bytes([body[2], body[3], body[4], body[5]]),
                value: u32::from_le_bytes([body[6], body[7], body[8], body[9]]),
            })
        }
        T_ACK => {
            want(6)?;
            Ok(Msg::Ack {
                topic: u16::from_le_bytes([body[0], body[1]]),
                key: u32::from_le_bytes([body[2], body[3], body[4], body[5]]),
            })
        }
        t => Err(DecodeError::Type(t)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_round_trips() {
        for m in [
            Msg::Hello { build: 0xBEEF },
            Msg::Ping { seq: 7 },
            Msg::Pong { seq: u32::MAX },
            Msg::Fact {
                topic: 0xFFFF,
                key: u32::MAX,
                value: 7,
            },
            Msg::Ack { topic: 1, key: 0 },
        ] {
            assert_eq!(decode(&encode(&m)), Ok(m));
        }
    }

    #[test]
    fn bad_input_is_refused_not_trusted() {
        assert_eq!(decode(&[]), Err(DecodeError::TooShort));
        assert_eq!(decode(&[VERSION]), Err(DecodeError::TooShort));
        assert_eq!(
            decode(&[9, T_PING, 0, 0, 0, 0]),
            Err(DecodeError::Version(9))
        );
        assert_eq!(decode(&[VERSION, 99]), Err(DecodeError::Type(99)));
        assert_eq!(
            decode(&[VERSION, T_PING, 1, 2, 3]),
            Err(DecodeError::Length { want: 4, got: 3 })
        );
        assert_eq!(
            decode(&[VERSION, T_PING, 1, 2, 3, 4, 5]),
            Err(DecodeError::Length { want: 4, got: 5 })
        );
        assert_eq!(
            decode(&[VERSION, T_FACT, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(DecodeError::Length { want: 10, got: 8 })
        );
        assert_eq!(
            decode(&[VERSION, T_ACK, 0, 0, 0, 0, 0, 0, 0]),
            Err(DecodeError::Length { want: 6, got: 7 })
        );
        assert_eq!(
            decode(&vec![VERSION; MAX_LEN + 1]),
            Err(DecodeError::TooLong)
        );
    }

    #[test]
    fn no_input_of_any_short_length_panics() {
        // Every byte string up to 3 bytes, every first two bytes of a bigger one: the decoder must return, never panic.
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                let _ = decode(&[a, b]);
                let _ = decode(&[a, b, 1, 2, 3, 4]);
                let _ = decode(&[a]);
            }
        }
    }
}
