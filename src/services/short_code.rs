// ---------------------------------------------------------------------------
// ShortCodeService
// ---------------------------------------------------------------------------

const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Generates random alphanumeric short codes of a fixed length.
pub struct ShortCodeService {
    length: u8,
}

impl ShortCodeService {
    pub fn new(length: u8) -> Self {
        ShortCodeService { length }
    }

    /// Return a random alphanumeric string of exactly `self.length` characters.
    ///
    /// Characters are drawn from [a-zA-Z0-9].
    pub fn generate(&self) -> String {
        (0..self.length)
            .map(|_| {
                let idx = rand::random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_returns_string_of_correct_length() {
        let service = ShortCodeService::new(4);
        let code = service.generate();
        assert_eq!(code.len(), 4, "expected code of length 4, got {:?}", code);
    }

    #[test]
    fn generate_respects_configured_length() {
        let service = ShortCodeService::new(8);
        let code = service.generate();
        assert_eq!(code.len(), 8, "expected code of length 8, got {:?}", code);
    }

    #[test]
    fn generate_returns_only_alphanumeric_characters() {
        let service = ShortCodeService::new(20);
        let code = service.generate();
        assert!(
            code.chars().all(|c| c.is_ascii_alphanumeric()),
            "expected all alphanumeric characters, got {:?}",
            code
        );
    }

    #[test]
    fn generate_returns_different_values_on_consecutive_calls() {
        let service = ShortCodeService::new(4);
        let a = service.generate();
        let b = service.generate();
        assert_ne!(a, b, "consecutive generate() calls must (almost certainly) differ");
    }
}
