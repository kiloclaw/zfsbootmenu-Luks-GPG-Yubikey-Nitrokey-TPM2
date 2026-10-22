//! The hashes and encodings of the key methods, computed in the process so
//! that secrets never reach `sha256sum`, `openssl` or `base64`, where they
//! would pass through pipes and other programs' memory, or arguments that any
//! process can read in `/proc` (see "Secrets" in `crates/DESIGN.md`).
//!
//! - SHA-256 (FIPS 180-4), for the TPM2 authorization value of a PIN and the
//!   `ykchalresp-hash` challenge;
//! - HMAC-SHA256 and PBKDF2-HMAC-SHA256 (RFC 2104, RFC 8018), for the salted
//!   PINs of systemd's TPM2 tokens;
//! - base64 (RFC 4648), the encoding of TPM2 secrets and token metadata;
//! - lower-case hexadecimal.
//!
//! Buffers that held secret material are wiped before they are freed.

use zbm_platform::secret::Secret;

/// The initial hash value of SHA-256.
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The round constants of SHA-256.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// The size of a SHA-256 block.
const BLOCK: usize = 64;

/// A SHA-256 computation.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; BLOCK],
    buffered: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Sha256 {
        Sha256::new()
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        zbm_sys::wipe(&mut self.buffer);
        self.state = [0; 8];
    }
}

impl Sha256 {
    /// A new computation.
    pub fn new() -> Sha256 {
        Sha256 {
            state: H0,
            buffer: [0; BLOCK],
            buffered: 0,
            length: 0,
        }
    }

    /// Adds `data`.
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (BLOCK - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered < BLOCK {
                return;
            }
            compress(&mut self.state, &self.buffer);
            self.buffered = 0;
        }
        let mut blocks = data.chunks_exact(BLOCK);
        for block in &mut blocks {
            compress(&mut self.state, block);
        }
        let rest = blocks.remainder();
        self.buffer[..rest.len()].copy_from_slice(rest);
        self.buffered = rest.len();
    }

    /// The hash of everything added.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != BLOCK - 8 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut digest = [0; 32];
        for (chunk, word) in digest.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }
}

/// The compression function of SHA-256 on one block.
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut w = [0_u32; 64];
    for (word, bytes) in w.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (k, word) in K.iter().zip(w) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choice = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(choice)
            .wrapping_add(*k)
            .wrapping_add(word);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (value, add) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *value = value.wrapping_add(add);
    }
    w.fill(0);
    std::hint::black_box(&w);
}

/// The SHA-256 hash of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finish()
}

/// HMAC-SHA256 of `message` with `key`, prepared once for many messages.
struct Hmac {
    inner: Sha256,
    outer: Sha256,
}

impl Hmac {
    fn new(key: &[u8]) -> Hmac {
        let mut block = [0_u8; BLOCK];
        if key.len() > BLOCK {
            block[..32].copy_from_slice(&sha256(key));
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut pad = [0_u8; BLOCK];
        let mut inner = Sha256::new();
        for (pad, byte) in pad.iter_mut().zip(block) {
            *pad = byte ^ 0x36;
        }
        inner.update(&pad);
        let mut outer = Sha256::new();
        for (pad, byte) in pad.iter_mut().zip(block) {
            *pad = byte ^ 0x5c;
        }
        outer.update(&pad);
        zbm_sys::wipe(&mut pad);
        zbm_sys::wipe(&mut block);
        Hmac { inner, outer }
    }

    fn mac(&self, message: &[u8]) -> [u8; 32] {
        let mut inner = self.inner.clone();
        inner.update(message);
        let mut digest = inner.finish();
        let mut outer = self.outer.clone();
        outer.update(&digest);
        zbm_sys::wipe(&mut digest);
        outer.finish()
    }
}

/// HMAC-SHA256 of `message` with `key`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    Hmac::new(key).mac(message)
}

/// PBKDF2-HMAC-SHA256 of `password` with `salt` and `rounds` iterations,
/// `length` bytes long.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], rounds: u32, length: usize) -> Secret {
    let hmac = Hmac::new(password);
    let mut key = Secret::new();
    let mut index = 1_u32;
    while key.len() < length {
        let mut message = salt.to_vec();
        message.extend_from_slice(&index.to_be_bytes());
        let mut u = hmac.mac(&message);
        let mut block = u;
        for _ in 1..rounds {
            u = hmac.mac(&u);
            for (byte, add) in block.iter_mut().zip(u) {
                *byte ^= add;
            }
        }
        let take = (length - key.len()).min(block.len());
        key.push_bytes(&block[..take]);
        zbm_sys::wipe(&mut u);
        zbm_sys::wipe(&mut block);
        index += 1;
    }
    key
}

/// The base64 alphabet.
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in base64 with padding, without line breaks, kept as a secret.
pub fn base64_encode(data: &[u8]) -> Secret {
    let mut text = Secret::new();
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let value = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                text.push(BASE64[((value >> shift) & 0x3f) as usize]);
            } else {
                text.push(b'=');
            }
        }
    }
    text
}

/// Decodes base64 text, ignoring line breaks, as `base64 -d` does; `None` if
/// the text is not base64.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut value = 0_u32;
    let mut bits = 0;
    let mut padding = 0;
    for byte in text.bytes().filter(|byte| !matches!(byte, b'\n' | b'\r')) {
        if byte == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        let digit = BASE64.iter().position(|&c| c == byte)?;
        value = value << 6 | u32::try_from(digit).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            data.push(u8::try_from((value >> bits) & 0xff).ok()?);
        }
    }
    (padding <= 2 && bits < 6).then_some(data)
}

/// `data` in lower-case hexadecimal, kept as a secret.
pub fn hex(data: &[u8]) -> Secret {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = Secret::new();
    for byte in data {
        text.push(DIGITS[usize::from(byte >> 4)]);
        text.push(DIGITS[usize::from(byte & 0x0f)]);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::exec::{Command, Input, Runner, Stream, SystemRunner};
    use zbm_testkit::{Rng, have_program};

    fn hex_text(data: &[u8]) -> String {
        String::from_utf8(hex(data).expose().to_vec()).unwrap()
    }

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            hex_text(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_text(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_text(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex_text(&sha256(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        // Fed in pieces of every size, the hash is the same.
        let data: Vec<u8> = (0..300_u16).map(|n| (n % 251) as u8).collect();
        for piece in [1, 3, 63, 64, 65, 128] {
            let mut hash = Sha256::new();
            for chunk in data.chunks(piece) {
                hash.update(chunk);
            }
            assert_eq!(hash.finish(), sha256(&data), "{piece}");
        }
    }

    #[test]
    fn hmac_and_pbkdf2_match_the_standard_vectors() {
        // RFC 4231, test cases 2 and 6.
        assert_eq!(
            hex_text(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hex_text(&hmac_sha256(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        // RFC 7914, section 11.
        let key = pbkdf2_hmac_sha256(b"passwd", b"salt", 1, 64);
        assert_eq!(
            hex_text(key.expose()),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
        );
        let key = pbkdf2_hmac_sha256(b"Password", b"NaCl", 80_000, 64);
        assert_eq!(
            hex_text(key.expose()),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
             a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d"
        );
    }

    #[test]
    fn base64_round_trips() {
        for (data, text) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(data).expose(), text.as_bytes());
            assert_eq!(base64_decode(text).as_deref(), Some(data));
        }
        assert_eq!(
            base64_decode("Zm9v\nYmFy\n").as_deref(),
            Some(&b"foobar"[..])
        );
        assert_eq!(base64_decode("Zm9v!"), None);
        assert_eq!(base64_decode("Zg==Zg"), None);
        assert_eq!(base64_decode("Z"), None);
    }

    /// Runs a program with `stdin` and returns its standard output.
    fn run(program: &str, args: &[&str], stdin: &[u8]) -> Vec<u8> {
        let command = Command::new(program)
            .args(args)
            .stdin(Input::Bytes(stdin.to_vec()))
            .stderr(Stream::Null);
        SystemRunner.run(&command).unwrap().stdout
    }

    #[test]
    fn results_match_the_programs_the_legacy_code_runs() {
        let mut rng = Rng::from_env(0x5a5a);
        for _ in 0..20 {
            let len = rng.below(200);
            let data: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
            if have_program("sha256sum") {
                let legacy = run("sha256sum", &[], &data);
                assert_eq!(hex(&sha256(&data)).expose(), &legacy[..64]);
            }
            if have_program("base64") {
                let legacy = run("base64", &[], &data);
                let legacy: Vec<u8> = legacy.into_iter().filter(|byte| *byte != b'\n').collect();
                assert_eq!(base64_encode(&data).expose(), legacy);
                let text = String::from_utf8(legacy).unwrap();
                assert_eq!(base64_decode(&text), Some(data.clone()));
            }
        }
        if !have_program("openssl") {
            return;
        }
        // The salted PIN of the legacy code: `openssl kdf ... PBKDF2`.
        for (pin, salt) in [
            ("1234", &b"0123456789abcdef"[..]),
            ("", b"s"),
            ("pïn é", b"\x00\xff"),
        ] {
            let hexsalt = String::from_utf8(hex(salt).expose().to_vec()).unwrap();
            let pass = format!("pass:{pin}");
            let salt_option = format!("hexsalt:{hexsalt}");
            let args = [
                "kdf",
                "-keylen",
                "32",
                "-kdfopt",
                "digest:SHA256",
                "-kdfopt",
                &pass,
                "-kdfopt",
                &salt_option,
                "-kdfopt",
                "iter:10000",
                "-binary",
                "PBKDF2",
            ];
            let legacy = run("openssl", &args, b"");
            if legacy.is_empty() {
                // An empty password cannot be given to openssl this way.
                continue;
            }
            let key = pbkdf2_hmac_sha256(pin.as_bytes(), salt, 10_000, 32);
            assert_eq!(key.expose(), legacy, "{pin:?}");
        }
    }
}
