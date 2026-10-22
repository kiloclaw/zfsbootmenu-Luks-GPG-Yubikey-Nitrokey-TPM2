//! LUKS2 tokens: what `cryptsetup luksDump --dump-json-metadata` and
//! `cryptsetup token export` print, as values.
//!
//! A token is an object in the LUKS2 header's JSON metadata, under `tokens`,
//! keyed by its ID: a `type` (such as `systemd-tpm2`), the `keyslots` it
//! opens, and fields of its type (`tpm2-blob`, `fido2-credential`, `jwe`).
//! [`Token`] keeps those fields and reads them with the JSON types they have.

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
        assert_eq!(token.text("tpm2-pubkey"), None);
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
