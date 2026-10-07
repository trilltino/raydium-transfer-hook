//! Template manifests: a JSON document describing a hook template off-chain, a canonical form of it,
//! and the content-derived id that names it.
//!
//! The id is `SHA-256(canonical manifest)`, so two parties who canonicalise the same manifest get
//! the same id without coordinating, and changing a manifest changes its id. Large prose belongs in
//! the manifest (or files it points at), never on-chain.
//!
//! # The manifest
//!
//! A JSON object with at least a string `name` and a string `version`. Everything else is free
//! (description, repository, rules, accounts, authors, ...). Numbers must be integers, so the
//! canonical form is exact.
//!
//! # Canonical form
//!
//! Objects with keys sorted by their bytes, no whitespace, arrays in order, strings escaped as
//! `serde_json` escapes them, integers in plain decimal. (It is the JSON Canonicalization Scheme
//! restricted to integers.)

use std::fmt;

use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManifestError {
    NotAnObject,
    MissingField(&'static str),
    /// A number that is not an integer: its canonical text would not be exact.
    FloatNotAllowed,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => f.write_str("a manifest must be a JSON object"),
            Self::MissingField(name) => write!(f, "a manifest needs a string `{name}`"),
            Self::FloatNotAllowed => f.write_str(
                "manifests may only contain integer numbers, so they canonicalise exactly",
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Check the required fields.
pub fn validate(manifest: &Value) -> Result<(), ManifestError> {
    let object = manifest.as_object().ok_or(ManifestError::NotAnObject)?;
    for field in ["name", "version"] {
        if !object.get(field).is_some_and(Value::is_string) {
            return Err(ManifestError::MissingField(match field {
                "name" => "name",
                _ => "version",
            }));
        }
    }
    Ok(())
}

fn write_canonical(value: &Value, out: &mut String) -> Result<(), ManifestError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_f64() {
                return Err(ManifestError::FloatNotAllowed);
            }
            out.push_str(&n.to_string());
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).expect("a string serialises")),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (i, (key, item)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("a key serialises"));
                out.push(':');
                write_canonical(item, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// The canonical text of a manifest.
pub fn canonical_json(manifest: &Value) -> Result<String, ManifestError> {
    let mut out = String::new();
    write_canonical(manifest, &mut out)?;
    Ok(out)
}

/// The content-derived template id: `SHA-256(canonical manifest)`. Validates the manifest first.
pub fn template_id(manifest: &Value) -> Result<[u8; 32], ManifestError> {
    validate(manifest)?;
    Ok(Sha256::digest(canonical_json(manifest)?.as_bytes()).into())
}

/// The hash a descriptor stores in `manifest_hash`: `SHA-256` of the manifest **document as
/// published** (its exact bytes), so a reader can check a copy they were handed.
pub fn manifest_hash(published: &[u8]) -> [u8; 32] {
    Sha256::digest(published).into()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn key_order_and_whitespace_do_not_change_the_id() {
        let a: Value =
            serde_json::from_str(r#"{ "name": "vesting", "version": "1", "rules": [1, 2] }"#)
                .unwrap();
        let b: Value =
            serde_json::from_str(r#"{"rules":[1,2],"version":"1","name":"vesting"}"#).unwrap();
        assert_eq!(template_id(&a), template_id(&b));
        assert_eq!(
            canonical_json(&a).unwrap(),
            r#"{"name":"vesting","rules":[1,2],"version":"1"}"#
        );
    }

    #[test]
    fn any_change_to_the_content_changes_the_id() {
        let base = json!({"name": "vesting", "version": "1"});
        let renamed = json!({"name": "vesting2", "version": "1"});
        let bumped = json!({"name": "vesting", "version": "2"});
        let extended = json!({"name": "vesting", "version": "1", "note": "x"});
        let ids: Vec<_> = [&base, &renamed, &bumped, &extended]
            .iter()
            .map(|m| template_id(m).unwrap())
            .collect();
        for i in 0..ids.len() {
            for j in i + 1..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn the_id_is_pinned_to_a_digest_computed_independently() {
        // `printf '%s' '{"name":"a","version":"1"}' | sha256sum`
        let id = template_id(&json!({"version": "1", "name": "a"})).unwrap();
        let hex: String = id.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "42867b1854781498f98172941ab3920fc1166968249e9189e1e31213a4a17e12"
        );
    }

    #[test]
    fn required_fields_and_integer_only_numbers_are_enforced() {
        assert_eq!(validate(&json!([1])), Err(ManifestError::NotAnObject));
        assert_eq!(
            validate(&json!({"version": "1"})),
            Err(ManifestError::MissingField("name"))
        );
        assert_eq!(
            validate(&json!({"name": "a", "version": 1})),
            Err(ManifestError::MissingField("version"))
        );
        assert_eq!(
            template_id(&json!({"name": "a", "version": "1", "x": 1.5})),
            Err(ManifestError::FloatNotAllowed)
        );
        assert!(template_id(&json!({"name": "a", "version": "1", "x": -3})).is_ok());
    }

    #[test]
    fn strings_are_escaped_so_they_cannot_forge_structure() {
        let tricky = json!({"name": "a\",\"version\":\"9", "version": "1"});
        let text = canonical_json(&tricky).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            tricky,
            "the canonical text must parse back to the same value"
        );
    }

    #[test]
    fn the_manifest_hash_is_the_hash_of_the_published_bytes() {
        let bytes = b"{ \"name\": \"a\", \"version\": \"1\" }\n";
        assert_eq!(
            manifest_hash(bytes).as_slice(),
            Sha256::digest(bytes).as_slice()
        );
        // Different bytes for the same manifest: same id, different hash.
        let compact = br#"{"name":"a","version":"1"}"#;
        assert_ne!(manifest_hash(bytes), manifest_hash(compact));
    }
}
