//! The canonical pack digest (CF-02, `canary-pack-v1`).
//!
//! A *pack digest* fingerprints the full content of a fixture pack —
//! every fixture file plus every externally referenced input/expected
//! file — as a single `sha256:<hex>` string. Two fixture directories
//! with identical content produce the same digest regardless of where
//! they live on disk, so a lockfile pinning the digest (see
//! `canary_config::lockfile`) is portable across machines and CI.
//!
//! The digest is content-addressed only: no absolute or relative paths,
//! no mtimes, and no directory-entry ordering influence the result.
//! Entries are sorted by `(surface, id)` and hashed over a canonical
//! tab-separated line per fixture:
//!
//! ```text
//! <surface>\t<id>\t<sha256 of fixture file>\t<sha256 of input file, or "-">\t<sha256 of expected file, or "-">\n
//! ```
//!
//! SHA-256 is implemented here (FIPS 180-4) rather than pulled in as a
//! dependency, so the digest stays reproducible with zero new crates.

use std::path::Path;

use crate::loader::FixtureError;
use crate::manifest::LoadedFixture;

/// The algorithm prefix of every pack digest produced by this module.
pub const PACK_DIGEST_ALGORITHM: &str = "sha256";

/// Computes the `canary-pack-v1` pack digest over `fixtures`.
///
/// Referenced input/expected files are read from disk (their paths were
/// already resolved relative to the fixture's own directory at parse
/// time). Returns a [`FixtureError`] if any file cannot be read or a
/// referenced file does not exist — a digest is never computed from a
/// partial view of the pack.
pub fn pack_digest(fixtures: &[LoadedFixture]) -> Result<String, FixtureError> {
    let mut entries: Vec<(&LoadedFixture, String)> = fixtures
        .iter()
        .map(|fixture| {
            let key = format!("{}\t{}", fixture.surface, fixture.metadata.id);
            (fixture, key)
        })
        .collect();
    // Sort by (surface, id) — both lowercase surface strings, so this is
    // the alphabetical surface order CF-02 specifies. Ties (duplicate ids
    // across surfaces were already rejected by `validate`) fall back to
    // the source path so the order is total.
    entries.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.source_path.cmp(&b.0.source_path)));

    let mut canonical = String::new();
    for (fixture, _) in entries {
        let fixture_hash = file_digest(&fixture.source_path)?;
        let input_hash = match &fixture.input_file {
            Some(path) => file_digest(path)?,
            None => "-".to_string(),
        };
        let expected_hash = match &fixture.expected_file {
            Some(path) => file_digest(path)?,
            None => "-".to_string(),
        };
        canonical.push_str(&format!(
            "{}\t{fixture_hash}\t{input_hash}\t{expected_hash}\n",
            fixture_key(fixture),
        ));
    }

    Ok(format!(
        "{PACK_DIGEST_ALGORITHM}:{}",
        sha256_hex(canonical.as_bytes())
    ))
}

/// The canonical `<surface>\t<id>` sort key for one fixture.
fn fixture_key(fixture: &LoadedFixture) -> String {
    format!("{}\t{}", fixture.surface, fixture.metadata.id)
}

/// Hashes a file's raw bytes with SHA-256, hex-encoded.
fn file_digest(path: &Path) -> Result<String, FixtureError> {
    let bytes = std::fs::read(path).map_err(|source| FixtureError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(sha256_hex(&bytes))
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4), implemented locally to avoid a new dependency.
// Verified by the known-answer tests at the bottom of this file.
// ---------------------------------------------------------------------------

/// Round constants: the first 32 bits of the fractional parts of the cube
/// roots of the first 64 primes.
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Incremental SHA-256 hasher.
#[derive(Clone)]
struct Sha256 {
    /// Chaining state H0..H7.
    state: [u32; 8],
    /// Partial block buffer.
    buffer: [u8; 64],
    /// Bytes currently buffered (0..64).
    buffer_len: usize,
    /// Total message bytes fed in so far, for the length suffix.
    total_len: u64,
}

impl Sha256 {
    fn new() -> Self {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.total_len = self.total_len.wrapping_add(data.len() as u64);

        // Top up a partially filled buffer first.
        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take]
                .copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        // Consume whole blocks straight from the input.
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }

        // Buffer the tail.
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    fn finalize(mut self) -> [u8; 32] {
        // The length suffix counts *message* bits, so capture it before
        // padding starts feeding extra bytes through `update`.
        let bit_len = self.total_len.wrapping_mul(8);

        self.update(&[0x80]);
        while self.buffer_len != 56 {
            self.update(&[0x00]);
        }
        self.update(&bit_len.to_be_bytes());
        debug_assert_eq!(self.buffer_len, 0, "padding must flush the last block");

        let mut out = [0u8; 32];
        for (chunk, word) in out.chunks_exact_mut(4).zip(self.state.iter()) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (word, chunk) in w.iter_mut().take(16).zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let x = w[i - 15];
            let y = w[i - 2];
            let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(big_s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big_s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

/// Hex-encoded SHA-256 of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_fips_known_answers() {
        // The canonical NIST known-answer vectors, so a typo in the
        // round constants or the compression logic cannot pass silently.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_hashes_streamed_input_the_same_as_one_shot() {
        // Exercises the partial-buffer path across block boundaries.
        let data: Vec<u8> = (0u32..300).map(|i| (i % 251) as u8).collect();
        let mut hasher = Sha256::new();
        for chunk in data.chunks(7) {
            hasher.update(chunk);
        }
        let streamed = hasher.finalize();
        let mut one_shot = Sha256::new();
        one_shot.update(&data);
        assert_eq!(streamed, one_shot);
    }

    #[test]
    fn pack_digest_is_prefixed_and_hex_encoded() {
        let dir = crate::test_support::temp_dir("pack-digest");
        let path = dir.path.join("fixture.toml");
        std::fs::write(
            &path,
            r#"
            id = "p28-xdr-001"
            protocol = 28
            surface = "xdr"
            category = "cap-83"
            description = "digest test"
            "#,
        )
        .unwrap();
        let loaded = crate::manifest::parse_fixture_file(&path).expect("parses");

        let digest = pack_digest(std::slice::from_ref(&loaded)).expect("digest");
        let hex = digest
            .strip_prefix("sha256:")
            .expect("digest must carry the algorithm prefix");
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn identical_content_in_two_directories_produces_the_same_digest() {
        let fixture_toml = r#"
            id = "p28-xdr-001"
            protocol = 28
            surface = "xdr"
            category = "cap-83"
            description = "digest test"
            "#;

        let dir_a = crate::test_support::temp_dir("pack-digest-a");
        let dir_b = crate::test_support::temp_dir("pack-digest-b");
        let path_a = dir_a.path.join("fixture.toml");
        let path_b = dir_b.path.join("nested").join("fixture.toml");
        std::fs::create_dir_all(path_b.parent().unwrap()).unwrap();
        std::fs::write(&path_a, fixture_toml).unwrap();
        std::fs::write(&path_b, fixture_toml).unwrap();

        let loaded_a = crate::manifest::parse_fixture_file(&path_a).expect("parses");
        let loaded_b = crate::manifest::parse_fixture_file(&path_b).expect("parses");

        assert_eq!(
            pack_digest(std::slice::from_ref(&loaded_a)).unwrap(),
            pack_digest(std::slice::from_ref(&loaded_b)).unwrap(),
            "the digest must be content-addressed, not path-addressed"
        );
    }
}
