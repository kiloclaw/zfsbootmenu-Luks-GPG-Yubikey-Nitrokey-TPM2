//! LUKS2 tokens: what `cryptsetup luksDump --dump-json-metadata` and
//! `cryptsetup token export` print, as values.
//!
//! A token is an object in the LUKS2 header's JSON metadata, under `tokens`,
//! keyed by its ID: a `type` (such as `systemd-tpm2`), the `keyslots` it
//! opens, and fields of its type (`tpm2-blob`, `fido2-credential`, `jwe`).
//! [`Token`] keeps those fields and reads them with the JSON types they have.
//! [`Token::argon2id`] reads the costs of a systemd 262 TPM2 PIN as
//! systemd's own parser does.

use std::fmt;

use serde_json::{Map, Value};

/// Why token metadata could not be read.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    /// The text is not JSON.
    #[error("the token metadata is not JSON: {0}")]
    Syntax(#[from] serde_json::Error),
    /// The JSON is not an object, or a token has no type.
    #[error("unexpected token metadata: {0}")]
    Shape(&'static str),
}

/// The Argon2id costs of a TPM2 PIN that systemd 262 hardened
/// (`systemd-cryptenroll --tpm2-with-pin=yes`), as the token records them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Argon2idFields {
    /// `tpm2_argon2id_memcost`: the memory, in bytes.
    pub memcost_bytes: u64,
    /// `tpm2_argon2id_iterations`: the passes over the memory.
    pub iterations: u32,
    /// `tpm2_argon2id_lanes`: the lanes the memory is split into.
    pub lanes: u32,
}

/// Why a token's Argon2id fields are not what systemd's parser accepts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BadField {
    /// The field is not an unsigned integer: a string, a negative number,
    /// a number with a fraction or above 2⁶⁴ − 1. A number written with
    /// a fraction or an exponent counts when its value is whole (`2.0`), as
    /// in systemd.
    #[error("{0} is not an unsigned integer")]
    NotUnsigned(&'static str),
    /// The field is 0.
    #[error("{0} is 0")]
    Zero(&'static str),
    /// The field is above 2³² − 1.
    #[error("{0} is larger than 4294967295")]
    TooLarge(&'static str),
    /// Some of the three fields are there, not all.
    #[error("the Argon2id fields are incomplete")]
    Incomplete,
}

/// The field of [`Argon2idFields::memcost_bytes`].
const MEMCOST: &str = "tpm2_argon2id_memcost";
/// The field of [`Argon2idFields::iterations`].
const ITERATIONS: &str = "tpm2_argon2id_iterations";
/// The field of [`Argon2idFields::lanes`].
const LANES: &str = "tpm2_argon2id_lanes";

/// A JSON number written with a fraction or an exponent whose value is a
/// whole number from 0 to 2⁶⁴ − 1 (`2.0`, `3.3e9`, `-0`), which systemd's
/// parser takes as that unsigned integer.
fn whole_number(value: &Value) -> Option<u64> {
    // 2⁶⁴, the first value above the range, exactly.
    const ABOVE: f64 = 18_446_744_073_709_551_616.0;
    let number = value.as_f64()?;
    let whole = number.fract() == 0.0 && (0.0..ABOVE).contains(&number);
    // The cast is exact: the number is whole and within u64's range.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    whole.then_some(number as u64)
}

/// A LUKS2 token: its ID, its type and its fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// The token ID, such as `0`.
    pub id: String,
    /// The type, such as `systemd-tpm2`, `systemd-fido2` or `clevis`.
    pub kind: String,
    /// Every field of the token's object, `type` and `keyslots` included.
    fields: Map<String, Value>,
}

impl Token {
    /// The token `id` from its JSON object, as `cryptsetup token export`
    /// prints it.
    ///
    /// # Errors
    ///
    /// [`TokenError`] when the text is not a JSON object with a `type`.
    pub fn from_export(id: &str, text: &str) -> Result<Token, TokenError> {
        match serde_json::from_str(text)? {
            Value::Object(fields) => Token::from_fields(id, fields),
            _ => Err(TokenError::Shape("a token is not an object")),
        }
    }

    fn from_fields(id: &str, fields: Map<String, Value>) -> Result<Token, TokenError> {
        let kind = fields
            .get("type")
            .and_then(Value::as_str)
            .ok_or(TokenError::Shape("a token has no type"))?
            .to_owned();
        Ok(Token {
            id: id.to_owned(),
            kind,
            fields,
        })
    }

    /// A token with only an ID and a type, from the text of `cryptsetup
    /// luksDump`, which shows no fields.
    pub fn without_fields(id: &str, kind: &str) -> Token {
        let mut fields = Map::new();
        fields.insert("type".to_owned(), Value::String(kind.to_owned()));
        Token {
            id: id.to_owned(),
            kind: kind.to_owned(),
            fields,
        }
    }

    /// Whether the token's fields are known: false for a token read from the
    /// `luksDump` text.
    pub fn has_fields(&self) -> bool {
        self.fields.len() > 1
    }

    /// The string field `key`, unless it is missing or empty.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }

    /// The boolean field `key`.
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.fields.get(key).and_then(Value::as_bool)
    }

    /// The array field `key`, each element as text (numbers in decimal);
    /// `None` when it is missing or not an array.
    pub fn list(&self, key: &str) -> Option<Vec<String>> {
        let items = self.fields.get(key)?.as_array()?;
        Some(
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
        )
    }

    /// The key slots the token opens.
    pub fn keyslots(&self) -> Vec<String> {
        self.list("keyslots").unwrap_or_default()
    }

    /// The Argon2id costs of the token's TPM2 PIN, read as systemd 262's
    /// token parser reads them, for every token: each field must be an
    /// unsigned integer other than 0, the iterations and the lanes at most
    /// 2³² − 1, and the three come together. `None` when none of them is
    /// there: the PIN, if any, is not hardened with Argon2id.
    ///
    /// # Errors
    ///
    /// [`BadField`] for the first field systemd would refuse, in its order:
    /// the memory, the iterations, the lanes, then whether all are there.
    pub fn argon2id(&self) -> Result<Option<Argon2idFields>, BadField> {
        let memcost = self.positive(MEMCOST)?;
        let iterations = self.positive_u32(ITERATIONS)?;
        let lanes = self.positive_u32(LANES)?;
        match (memcost, iterations, lanes) {
            (Some(memcost_bytes), Some(iterations), Some(lanes)) => Ok(Some(Argon2idFields {
                memcost_bytes,
                iterations,
                lanes,
            })),
            (None, None, None) => Ok(None),
            _ => Err(BadField::Incomplete),
        }
    }

    /// The field `key`, which must be an unsigned integer other than 0
    /// when it is there.
    fn positive(&self, key: &'static str) -> Result<Option<u64>, BadField> {
        let Some(value) = self.fields.get(key) else {
            return Ok(None);
        };
        match value.as_u64().or_else(|| whole_number(value)) {
            None => Err(BadField::NotUnsigned(key)),
            Some(0) => Err(BadField::Zero(key)),
            Some(number) => Ok(Some(number)),
        }
    }

    /// The field `key`, as [`Token::positive`] reads it, at most 2³² − 1.
    fn positive_u32(&self, key: &'static str) -> Result<Option<u32>, BadField> {
        self.positive(key)?
            .map(|number| u32::try_from(number).map_err(|_| BadField::TooLarge(key)))
            .transpose()
    }

    /// The token as JSON, as `cryptsetup token export` prints it.
    pub fn to_json(&self) -> String {
        Value::Object(self.fields.clone()).to_string()
    }
}

impl fmt::Display for Token {
    /// The ID and the type, as `cryptsetup luksDump` lists a token:
    /// `0 systemd-tpm2`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.id, self.kind)
    }
}

/// The tokens in the JSON metadata of `cryptsetup luksDump
/// --dump-json-metadata`, in the order of their IDs.
///
/// # Errors
///
/// [`TokenError`] when the text is not JSON, or has a token without a type.
pub fn from_metadata(text: &str) -> Result<Vec<Token>, TokenError> {
    let Value::Object(mut metadata) = serde_json::from_str(text)? else {
        return Err(TokenError::Shape("the metadata is not an object"));
    };
    let Some(Value::Object(tokens)) = metadata.remove("tokens") else {
        return Ok(Vec::new());
    };
    let mut tokens: Vec<(u64, Token)> = tokens
        .into_iter()
        .map(|(id, value)| match value {
            Value::Object(fields) => Ok((
                id.parse().unwrap_or(u64::MAX),
                Token::from_fields(&id, fields)?,
            )),
            _ => Err(TokenError::Shape("a token is not an object")),
        })
        .collect::<Result<_, _>>()?;
    tokens.sort_by_key(|(order, _)| *order);
    Ok(tokens.into_iter().map(|(_, token)| token).collect())
}

/// Whether `c` is in awk's `[[:space:]]`.
fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// The tokens in the text of `cryptsetup luksDump`, for cryptsetup before
/// 2.4, which cannot print the JSON metadata: the lines `  <id>: <type>`
/// below `Tokens:`. Their fields are not known.
pub fn from_dump_text(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut in_tokens = false;
    for line in text.lines() {
        if line.starts_with("Tokens:") {
            in_tokens = true;
            continue;
        }
        if line.chars().next().is_some_and(|c| !is_space(c)) {
            in_tokens = false;
        }
        let Some(rest) = line.strip_prefix("  ") else {
            continue;
        };
        let after = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        let id = rest.strip_suffix(after).unwrap_or_default();
        let Some(after) = after.strip_prefix(": ") else {
            continue;
        };
        if !in_tokens || id.is_empty() {
            continue;
        }
        let kind = after
            .split(is_space)
            .find(|field| !field.is_empty())
            .unwrap_or_default();
        tokens.push(Token::without_fields(id, kind));
    }
    tokens
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "the tests read recorded fixtures"
)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../zbm-luks/tests/fixtures")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    fn listed(tokens: &[Token]) -> Vec<(String, String)> {
        tokens
            .iter()
            .map(|token| (token.id.clone(), token.kind.clone()))
            .collect()
    }

    #[test]
    fn the_json_metadata_and_the_dump_text_list_the_same_tokens() {
        for volume in ["tpm2", "three", "gap", "plain"] {
            let from_json = from_metadata(&fixture(&format!("luksdump-{volume}.json"))).unwrap();
            let from_text = from_dump_text(&fixture(&format!("luksdump-{volume}.txt")));
            assert_eq!(listed(&from_json), listed(&from_text), "{volume}");
            assert!(from_json.iter().all(Token::has_fields), "{volume}");
            assert!(!from_text.iter().any(Token::has_fields), "{volume}");
        }
        let three = from_metadata(&fixture("luksdump-three.json")).unwrap();
        assert!(three.len() > 1);
        assert!(
            from_metadata(&fixture("luksdump-plain.json"))
                .unwrap()
                .is_empty()
        );
        assert!(from_metadata(&fixture("luksdump-luks1.json")).is_err());
    }

    #[test]
    fn fields_are_read_with_their_json_types() {
        let token = Token::from_export("0", &fixture("token-tpm2.json")).unwrap();

        assert_eq!(token.kind, "systemd-tpm2");
        assert_eq!(token.to_string(), "0 systemd-tpm2");
        assert_eq!(token.keyslots(), ["1"]);
        assert_eq!(token.flag("tpm2-pin"), Some(false));
        assert_eq!(token.flag("tpm2-blob"), None);
        assert!(
            token
                .text("tpm2-blob")
                .is_some_and(|blob| blob.starts_with("AJ4A"))
        );
        assert_eq!(token.text("tpm2_pubkey"), None);
        assert_eq!(token.list("tpm2-pcrs"), Some(Vec::new()));
        assert_eq!(token.list("tpm2-blob"), None);
        let again = Token::from_export("0", &token.to_json()).unwrap();
        assert_eq!(again, token);
    }

    #[test]
    fn numbers_in_lists_read_as_decimal_text() {
        let token = Token::from_export(
            "2",
            r#"{"type":"systemd-tpm2","keyslots":["3"],"tpm2-pcrs":[0,7],"empty":""}"#,
        )
        .unwrap();

        assert_eq!(token.list("tpm2-pcrs").unwrap(), ["0", "7"]);
        assert_eq!(token.text("empty"), None);
    }

    #[test]
    fn malformed_metadata_is_an_error() {
        assert!(Token::from_export("0", "[]").is_err());
        assert!(Token::from_export("0", r#"{"keyslots":[]}"#).is_err());
        assert!(Token::from_export("0", "not json").is_err());
        assert!(from_metadata("[]").is_err());
        assert!(from_metadata(r#"{"tokens":{"0":[]}}"#).is_err());
        assert!(from_metadata(r#"{"keyslots":{}}"#).unwrap().is_empty());
        let ordered = from_metadata(r#"{"tokens":{"10":{"type":"b"},"2":{"type":"a"}}}"#).unwrap();
        assert_eq!(listed(&ordered)[0].0, "2");
    }

    /// The Argon2id fields of a token with `fields` added to a PIN token.
    fn argon2id_of(fields: &str) -> Result<Option<Argon2idFields>, BadField> {
        let json = format!(r#"{{"type":"systemd-tpm2","tpm2-pin":true{fields}}}"#);
        Token::from_export("0", &json).unwrap().argon2id()
    }

    #[test]
    fn argon2id_fields_follow_systemd_s_parser() {
        let all = |memcost: &str, iterations: &str, lanes: &str| {
            argon2id_of(&format!(
                r#","{MEMCOST}":{memcost},"{ITERATIONS}":{iterations},"{LANES}":{lanes}"#
            ))
        };
        assert_eq!(
            all("3334601937", "2", "24"),
            Ok(Some(Argon2idFields {
                memcost_bytes: 3_334_601_937,
                iterations: 2,
                lanes: 24,
            }))
        );
        let max = u64::MAX.to_string();
        assert_eq!(
            all(&max, "4294967295", "4294967295").map(|fields| fields.map(|f| f.memcost_bytes)),
            Ok(Some(u64::MAX))
        );
        // A whole number written with a fraction or an exponent is that
        // integer to systemd; -0 is 0.
        assert_eq!(
            all("3.3e9", "2.0", "24.000").map(|fields| fields.map(|f| (
                f.memcost_bytes,
                f.iterations,
                f.lanes
            ))),
            Ok(Some((3_300_000_000, 2, 24)))
        );
        assert_eq!(
            all("3334601937.0", "2", "24").map(|fields| fields.map(|f| f.memcost_bytes)),
            Ok(Some(3_334_601_937))
        );
        assert_eq!(all("-0", "2", "24"), Err(BadField::Zero(MEMCOST)));
        assert_eq!(all("0.0", "2", "24"), Err(BadField::Zero(MEMCOST)));
        assert_eq!(
            all("1", "4.294967296e9", "24"),
            Err(BadField::TooLarge(ITERATIONS))
        );
        assert_eq!(all("0", "2", "24"), Err(BadField::Zero(MEMCOST)));
        assert_eq!(all("1", "0", "24"), Err(BadField::Zero(ITERATIONS)));
        assert_eq!(all("1", "2", "0"), Err(BadField::Zero(LANES)));
        for bad in [
            "-1",
            "1.5",
            "\"7\"",
            "18446744073709551616",
            "1.8446744073709552e19",
            "1e300",
            "-1.0",
            "-2e3",
            "null",
            "true",
            "[1]",
        ] {
            assert_eq!(
                all(bad, "2", "24"),
                Err(BadField::NotUnsigned(MEMCOST)),
                "{bad}"
            );
            assert_eq!(all("1", bad, "24"), Err(BadField::NotUnsigned(ITERATIONS)));
            assert_eq!(all("1", "2", bad), Err(BadField::NotUnsigned(LANES)));
        }
        assert_eq!(
            all("1", "4294967296", "24"),
            Err(BadField::TooLarge(ITERATIONS))
        );
        assert_eq!(all("1", "2", "4294967296"), Err(BadField::TooLarge(LANES)));
        // systemd checks the fields in this order.
        assert_eq!(all("0", "0", "0"), Err(BadField::Zero(MEMCOST)));
        assert_eq!(all("1", "-1", "0"), Err(BadField::NotUnsigned(ITERATIONS)));

        let partial = [
            format!(r#","{MEMCOST}":1"#),
            format!(r#","{ITERATIONS}":1"#),
            format!(r#","{LANES}":1"#),
            format!(r#","{MEMCOST}":1,"{ITERATIONS}":1"#),
            format!(r#","{MEMCOST}":1,"{LANES}":1"#),
            format!(r#","{ITERATIONS}":1,"{LANES}":1"#),
        ];
        for fields in partial {
            assert_eq!(argon2id_of(&fields), Err(BadField::Incomplete), "{fields}");
        }
        // A bad field is named before the set is found incomplete.
        assert_eq!(
            argon2id_of(&format!(r#","{LANES}":0"#)),
            Err(BadField::Zero(LANES))
        );
        assert_eq!(argon2id_of(""), Ok(None));
        assert_eq!(
            BadField::Zero(LANES).to_string(),
            "tpm2_argon2id_lanes is 0"
        );
    }

    #[test]
    fn dump_text_lines_outside_the_token_section_are_ignored() {
        let text =
            "Keyslots:\n  0: luks2\nTokens:\n  0: clevis\n  x: no\n  1:no\nDigests:\n  0: pbkdf2\n";
        assert_eq!(
            listed(&from_dump_text(text)),
            [("0".to_owned(), "clevis".to_owned())]
        );
    }
}
