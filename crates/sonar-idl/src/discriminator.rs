use sha2::{Digest, Sha256};

/// Namespace for instruction discriminators.
pub const NAMESPACE_GLOBAL: &str = "global";

/// Namespace for event discriminators.
pub const NAMESPACE_EVENT: &str = "event";

/// Namespace for account type discriminators.
pub const NAMESPACE_ACCOUNT: &str = "account";

/// Discriminator for Anchor CPI event instructions.
pub const CPI_EVENT_DISCRIMINATOR: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Length of Anchor discriminators in bytes.
pub const DISCRIMINATOR_LEN: usize = 8;

/// An 8-byte Anchor discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discriminator(pub [u8; DISCRIMINATOR_LEN]);

impl Discriminator {
    /// Create a discriminator from a byte slice.
    ///
    /// Returns `None` if the slice is not exactly 8 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != DISCRIMINATOR_LEN {
            return None;
        }
        let mut arr = [0u8; DISCRIMINATOR_LEN];
        arr.copy_from_slice(bytes);
        Some(Self(arr))
    }

    /// Get the bytes of the discriminator.
    pub const fn as_bytes(&self) -> &[u8; DISCRIMINATOR_LEN] {
        &self.0
    }

    /// Get the bytes as a slice.
    pub const fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8]> for Discriminator {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Computes the 8-byte Anchor discriminator for a given namespace and name.
///
/// The discriminator is the first 8 bytes of `SHA256("namespace:name")`.
/// Common namespaces: [`NAMESPACE_GLOBAL`] (instructions), [`NAMESPACE_EVENT`] (events),
/// [`NAMESPACE_ACCOUNT`] (account types).
pub fn sighash(namespace: &str, name: &str) -> Discriminator {
    let preimage = format!("{}:{}", namespace, name);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; DISCRIMINATOR_LEN];
    out.copy_from_slice(&result[..DISCRIMINATOR_LEN]);
    Discriminator(out)
}

/// Convert camelCase to snake_case.
pub(crate) fn to_snake_case(s: &str) -> String {
    let mut snake = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                snake.push('_');
            }
            snake.push(c.to_ascii_lowercase());
        } else {
            snake.push(c);
        }
    }
    snake
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sighash_global_initialize() {
        let disc = sighash(NAMESPACE_GLOBAL, "initialize");
        assert_eq!(disc.as_bytes(), &[175, 175, 109, 31, 13, 152, 155, 237]);
    }

    #[test]
    fn sighash_account_discriminator() {
        let disc = sighash(NAMESPACE_ACCOUNT, "NewAccount");
        assert_eq!(disc.as_bytes().len(), 8);
        assert_ne!(disc.as_bytes(), &[0u8; 8]);
    }

    #[test]
    fn sighash_event_discriminator() {
        let disc = sighash(NAMESPACE_EVENT, "MyEvent");
        assert_eq!(disc.as_bytes().len(), 8);
        assert_ne!(disc.as_bytes(), sighash(NAMESPACE_GLOBAL, "MyEvent").as_bytes());
    }

    #[test]
    fn to_snake_case_camel() {
        assert_eq!(to_snake_case("createAccount"), "create_account");
    }

    #[test]
    fn to_snake_case_already_snake() {
        assert_eq!(to_snake_case("transfer"), "transfer");
    }

    #[test]
    fn to_snake_case_pascal() {
        assert_eq!(to_snake_case("InitializeAccount"), "initialize_account");
    }

    #[test]
    fn discriminator_from_bytes() {
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
        let disc = Discriminator::from_bytes(&bytes).unwrap();
        assert_eq!(disc.as_bytes(), &bytes);

        assert!(Discriminator::from_bytes(&[1, 2, 3]).is_none());
        assert!(Discriminator::from_bytes(&[1; 9]).is_none());
    }
}
