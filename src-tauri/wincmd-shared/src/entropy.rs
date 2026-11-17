// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure Shannon-entropy helpers for the ransomware monitor.
//!
//! Encrypted or compressed data is close to 8 bits per byte, while ordinary
//! documents sit well below. A sharp rise between a "before" and an "after"
//! sample of the same file is a ransomware signal. No I/O, no side effects.

/// Shannon entropy of `data` in bits per byte, in `0.0..=8.0`.
/// An empty buffer has entropy `0.0`.
pub fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0u64; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let len = data.len() as f64;
    let h: f64 = counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum();
    // Guard against -0.0 and tiny floating-point overshoot.
    h.clamp(0.0, 8.0)
}

/// Entropy change from `before` to `after` in bits per byte
/// (positive means the content became more random).
pub fn entropy_delta(before: &[u8], after: &[u8]) -> f64 {
    shannon_entropy(after) - shannon_entropy(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Deterministic pseudo-random bytes (xorshift64*), uniform enough for
    /// an entropy near 8 over a large buffer.
    fn pseudo_random(n: usize) -> Vec<u8> {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        (0..n)
            .map(|_| {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56) as u8
            })
            .collect()
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(shannon_entropy(&[]), 0.0);
        assert_eq!(entropy_delta(&[], &[]), 0.0);
    }

    #[test]
    fn single_repeated_byte_is_zero() {
        assert_eq!(shannon_entropy(&[0x41; 4096]), 0.0);
    }

    #[test]
    fn two_equal_symbols_is_one_bit() {
        let data: Vec<u8> = (0..1000).map(|i| (i % 2) as u8).collect();
        assert!(close(shannon_entropy(&data), 1.0));
    }

    #[test]
    fn uniform_all_byte_values_is_eight_bits() {
        let data: Vec<u8> = (0..=255u8).cycle().take(256 * 16).collect();
        assert!(close(shannon_entropy(&data), 8.0));
    }

    #[test]
    fn random_data_is_near_eight_bits() {
        let h = shannon_entropy(&pseudo_random(1 << 16));
        assert!(h > 7.99 && h <= 8.0, "entropy was {h}");
    }

    #[test]
    fn delta_flags_plaintext_becoming_random() {
        let plain = b"the quick brown fox jumps over the lazy dog ".repeat(200);
        let encrypted = pseudo_random(plain.len());
        let d = entropy_delta(&plain, &encrypted);
        assert!(d > 3.0, "delta was {d}");
        assert!(entropy_delta(&encrypted, &plain) < -3.0);
    }

    #[test]
    fn delta_of_identical_samples_is_zero() {
        let data = pseudo_random(4096);
        assert_eq!(entropy_delta(&data, &data), 0.0);
    }
}
