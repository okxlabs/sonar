use sha2::{Digest, Sha256};

/// Computes the 8-byte Anchor discriminator for a given namespace and name.
///
/// The discriminator is the first 8 bytes of `SHA256("namespace:name")`.
/// Common namespaces: `"global"` (instructions), `"event"` (events),
/// `"account"` (account types).
pub fn sighash(namespace: &str, name: &str) -> [u8; 8] {
    let preimage = format!("{}:{}", namespace, name);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&result[..8]);
    out
}

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
        let disc = sighash("global", "initialize");
        assert_eq!(disc, [175, 175, 109, 31, 13, 152, 155, 237]);
    }

    #[test]
    fn sighash_account_discriminator() {
        let disc = sighash("account", "NewAccount");
        assert_eq!(disc.len(), 8);
        assert_ne!(disc, [0u8; 8]);
    }

    #[test]
    fn sighash_event_discriminator() {
        let disc = sighash("event", "MyEvent");
        assert_eq!(disc.len(), 8);
        assert_ne!(disc, sighash("global", "MyEvent"));
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
}
