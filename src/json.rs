use crate::error::{Result, message};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, value::RawValue};
use std::{
    collections::{BTreeMap, HashSet},
    fmt,
};
pub type Object = BTreeMap<String, Box<RawValue>>;
struct Checked;
impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        d.deserialize_any(CheckVisitor)
    }
}
struct CheckVisitor;
impl<'de> Visitor<'de> for CheckVisitor {
    type Value = Checked;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("one JSON value without duplicate keys")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_str<E: de::Error>(self, _: &str) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Checked, E> {
        Ok(Checked)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Checked, A::Error> {
        while seq.next_element::<Checked>()?.is_some() {}
        Ok(Checked)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Checked, A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("duplicate JSON key {key:?}")));
            }
            map.next_value::<Checked>()?;
        }
        Ok(Checked)
    }
}
pub fn validate(data: &[u8]) -> Result<()> {
    std::str::from_utf8(data).map_err(|_| message("JSON contains invalid UTF-8"))?;
    let mut d = serde_json::Deserializer::from_slice(data);
    Checked::deserialize(&mut d)?;
    d.end()?;
    Ok(())
}
pub fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T> {
    validate(data)?;
    Ok(serde_json::from_slice(data)?)
}
pub fn object(data: &[u8]) -> Result<Object> {
    decode(data)
}
pub fn raw(value: &impl serde::Serialize) -> Result<Box<RawValue>> {
    Ok(serde_json::value::to_raw_value(value)?)
}
pub fn value(fields: &Object, key: &str) -> Result<Value> {
    fields
        .get(key)
        .map_or(Ok(Value::Null), |v| Ok(serde_json::from_str(v.get())?))
}
pub fn same(a: &RawValue, b: &RawValue) -> bool {
    let (Ok(a), Ok(b)) = (
        serde_json::from_str::<Value>(a.get()),
        serde_json::from_str::<Value>(b.get()),
    ) else {
        return false;
    };
    same_value(&a, &b)
}
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => decimal(&a.to_string()) == decimal(&b.to_string()),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, a)| b.get(k).is_some_and(|b| same_value(a, b)))
        }
        _ => a == b,
    }
}
fn decimal(text: &str) -> (String, num_bigint::BigInt) {
    let lower = text.to_ascii_lowercase();
    let (mantissa, exponent) = lower.split_once('e').unwrap_or((&lower, "0"));
    let mut exponent = exponent
        .parse::<num_bigint::BigInt>()
        .expect("validated JSON exponent");
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let joined = format!("{integer}{fraction}");
    let digits = joined.trim_start_matches('0');
    if digits.is_empty() {
        return ("0".into(), 0.into());
    }
    let coefficient = digits.trim_end_matches('0');
    exponent -= num_bigint::BigInt::from(fraction.len());
    exponent += num_bigint::BigInt::from(digits.len() - coefficient.len());
    (
        format!("{}{coefficient}", if negative { "-" } else { "" }),
        exponent,
    )
}
pub fn page_ids(fields: &Object) -> Result<Vec<String>> {
    let legacy: Vec<String> = match value(fields, "pages")? {
        Value::Null => Vec::new(),
        v => serde_json::from_value(v)?,
    };
    let mut ids = legacy.clone();
    if let Some(cpages) = fields.get("cPages") {
        let object = object(cpages.get().as_bytes())?;
        let rows: Vec<Object> = decode(
            object
                .get("pages")
                .ok_or_else(|| message("missing cPages.pages"))?
                .get()
                .as_bytes(),
        )?;
        if !rows.is_empty() {
            ids = Vec::new();
            for row in rows {
                let id: String = serde_json::from_value(value(&row, "id")?)?;
                if let Some(deleted) = row.get("deleted") {
                    let deleted = object_fn(deleted.get().as_bytes())?;
                    let v: i64 = serde_json::from_value(value(&deleted, "value")?)?;
                    if v != 0 {
                        return Err(message("native page is deleted or incompatible"));
                    }
                }
                ids.push(id);
            }
            if !legacy.is_empty() && legacy != ids {
                return Err(message("native page schemas disagree"));
            }
        }
    }
    let count: i64 = match fields.get("pageCount") {
        None => 0,
        Some(raw) => serde_json::from_str(raw.get())?,
    };
    if count < 0 || ids.is_empty() || (count > 0 && count as usize != ids.len()) {
        return Err(message(
            "native page IDs are incomplete; open the document on the tablet and sync first",
        ));
    }
    let mut seen = HashSet::new();
    for id in &ids {
        if !crate::validation::safe_native_path(id) || id.contains('/') || !seen.insert(id) {
            return Err(message("invalid or duplicate native page ID"));
        }
    }
    Ok(ids)
}
fn object_fn(data: &[u8]) -> Result<Object> {
    object(data)
}
