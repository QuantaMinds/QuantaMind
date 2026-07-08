//! Encryption-at-rest SEAM (laid, not implemented — see docs/security.md).
//!
//! The OSS build ships `Passthrough`: bytes are written unchanged. The separate enterprise
//! product swaps in a real keyed AEAD behind this trait so on-disk transcripts/history become
//! ciphertext WITHOUT touching any call site. This is a no-op today by design — for a
//! single-user local tool, an attacker with the disk already has the keychain, so at-rest
//! encryption earns its place only in the multi-tenant/enterprise context.
//!
//! Attach point: `persistence::jobs::transcripts` seals each appended line via `seal` and the
//! reader opens via `open`. The enterprise cipher must therefore be a PER-LINE AEAD (the
//! transcript is append-only JSONL), and the reader applies `open` per line.

use crate::errors::AppResult;

/// The at-rest cipher boundary. `seal` runs before bytes hit disk; `open` reverses it on read.
pub trait AtRest: Send + Sync {
    fn seal(&self, plaintext: &[u8]) -> Vec<u8>;
    fn open(&self, stored: &[u8]) -> AppResult<Vec<u8>>;
}

/// The OSS no-op: bytes pass through unchanged (nothing is encrypted at rest).
pub struct Passthrough;

impl AtRest for Passthrough {
    fn seal(&self, plaintext: &[u8]) -> Vec<u8> {
        plaintext.to_vec()
    }
    fn open(&self, stored: &[u8]) -> AppResult<Vec<u8>> {
        Ok(stored.to_vec())
    }
}

/// The active at-rest cipher. OSS build → `Passthrough`; the enterprise build returns a keyed
/// AEAD from the same accessor, so callers never change.
pub fn at_rest() -> impl AtRest {
    Passthrough
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_round_trips_unchanged() {
        let c = Passthrough;
        let data = b"{\"step\":1}";
        let sealed = c.seal(data);
        // The no-op is EXACTLY identity — this pins that it isn't silently transforming data
        // (so it's never mistaken for a real, implemented cipher).
        assert_eq!(sealed, data);
        assert_eq!(c.open(&sealed).unwrap(), data);
    }
}
