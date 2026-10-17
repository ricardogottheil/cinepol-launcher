//! Maps the `servers.dat` NBT tree to the domain `ServerList` and back.
//!
//! Known fields are read from and written into each entry compound; every other
//! tag becomes a `PreservedField` whose `kind` is the NBT tag id and whose
//! `payload` is the tag encoded as a one-field NBT compound, so saving restores
//! the exact `Value`.
//!
//! Two presence details are kept in `preserved` too, so a load-then-save leaves
//! the file unchanged:
//! - `hidden`: the raw tag is preserved when the file has one. On save the domain
//!   value always overrides it; `hidden` is written when it is true or was present.
//! - An entry whose `ip` is missing or does not parse as a `ServerAddress` cannot
//!   be a domain `ServerEntry`. It is kept whole in `ServerList::preserved` with
//!   kind [`RAW_ENTRY_KIND`] and its original index as the name, hidden from the
//!   domain list, and written back at that index (clamped to the list length).

use std::collections::HashMap;

use fastnbt::Value;

use crate::domain::server_list::{
    Preserved, PreservedField, ServerAddress, ServerEntry, ServerList, ServerListStoreError,
};

const SERVERS: &str = "servers";
const NAME: &str = "name";
const IP: &str = "ip";
const ICON: &str = "icon";
const ACCEPT_TEXTURES: &str = "acceptTextures";
const HIDDEN: &str = "hidden";
const PAYLOAD_KEY: &str = "value";

const TAG_COMPOUND: u8 = 10;
/// Outside the NBT tag id range (0..=12): a whole server entry the domain cannot hold.
const RAW_ENTRY_KIND: u8 = 0x80 | TAG_COMPOUND;

pub fn decode(bytes: &[u8]) -> Result<ServerList, ServerListStoreError> {
    let root: Value =
        fastnbt::from_bytes(bytes).map_err(|error| corrupt(format!("malformed NBT: {error}")))?;
    let Value::Compound(mut root) = root else {
        return Err(corrupt("the root tag is not a compound"));
    };

    let mut list = ServerList::default();
    let mut raw_entries = Vec::new();
    if let Some(servers) = root.remove(SERVERS) {
        let Value::List(items) = servers else {
            return Err(corrupt("`servers` is not a list"));
        };
        for (index, item) in items.into_iter().enumerate() {
            let Value::Compound(fields) = item else {
                return Err(corrupt("a `servers` item is not a compound"));
            };
            match decode_entry(fields)? {
                Ok(entry) => list.add(entry),
                Err(fields) => raw_entries.push(PreservedField {
                    name: index.to_string(),
                    kind: RAW_ENTRY_KIND,
                    payload: encode_payload(&Value::Compound(fields))?,
                }),
            }
        }
    }
    list.preserved = preserve(root)?;
    list.preserved.0.extend(raw_entries);
    Ok(list)
}

pub fn encode(list: &ServerList) -> Result<Vec<u8>, ServerListStoreError> {
    let mut servers: Vec<Value> = list
        .entries()
        .iter()
        .map(encode_entry)
        .collect::<Result<_, _>>()?;

    let mut root = HashMap::new();
    let mut raw_entries = Vec::new();
    for field in &list.preserved.0 {
        if field.kind == RAW_ENTRY_KIND {
            let index = field
                .name
                .parse::<usize>()
                .map_err(|_| corrupt("a kept server entry has no index"))?;
            raw_entries.push((
                index,
                decode_payload(field.payload.as_slice(), TAG_COMPOUND)?,
            ));
        } else {
            root.insert(field.name.clone(), restore(field)?);
        }
    }
    raw_entries.sort_by_key(|(index, _)| *index);
    for (index, entry) in raw_entries {
        servers.insert(index.min(servers.len()), entry);
    }

    root.insert(SERVERS.to_owned(), Value::List(servers));
    fastnbt::to_bytes(&Value::Compound(root))
        .map_err(|error| corrupt(format!("cannot encode NBT: {error}")))
}

/// `Ok` for a domain entry, `Err` with the untouched fields when the ip is unusable.
fn decode_entry(
    mut fields: HashMap<String, Value>,
) -> Result<Result<ServerEntry, HashMap<String, Value>>, ServerListStoreError> {
    let address = match fields.get(IP) {
        Some(Value::String(ip)) => ServerAddress::parse(ip).ok(),
        Some(_) => return Err(corrupt("a server `ip` is not a string")),
        None => None,
    };
    let Some(address) = address else {
        tracing::warn!("keeping a server entry with an unusable address as raw data");
        return Ok(Err(fields));
    };
    fields.remove(IP);

    let name = match fields.remove(NAME) {
        Some(Value::String(name)) => name,
        Some(_) => return Err(corrupt("a server `name` is not a string")),
        None => String::new(),
    };
    let icon = match fields.remove(ICON) {
        Some(Value::String(icon)) => Some(icon),
        Some(_) => return Err(corrupt("a server `icon` is not a string")),
        None => None,
    };
    let accept_textures = match fields.remove(ACCEPT_TEXTURES) {
        Some(Value::Byte(flag)) => Some(flag != 0),
        Some(_) => return Err(corrupt("a server `acceptTextures` is not a byte")),
        None => None,
    };
    // `hidden` stays in the preserved fields so its presence survives a save.
    let hidden = match fields.get(HIDDEN) {
        Some(Value::Byte(flag)) => *flag != 0,
        Some(_) => return Err(corrupt("a server `hidden` is not a byte")),
        None => false,
    };

    let mut entry = ServerEntry::new(name, address);
    entry.icon = icon;
    entry.accept_textures = accept_textures;
    entry.hidden = hidden;
    entry.preserved = preserve(fields)?;
    Ok(Ok(entry))
}

fn encode_entry(entry: &ServerEntry) -> Result<Value, ServerListStoreError> {
    let mut fields = HashMap::new();
    for field in &entry.preserved.0 {
        fields.insert(field.name.clone(), restore(field)?);
    }
    fields.insert(NAME.to_owned(), Value::String(entry.name.clone()));
    fields.insert(IP.to_owned(), Value::String(entry.address.to_string()));
    match &entry.icon {
        Some(icon) => fields.insert(ICON.to_owned(), Value::String(icon.clone())),
        None => fields.remove(ICON),
    };
    match entry.accept_textures {
        Some(flag) => fields.insert(ACCEPT_TEXTURES.to_owned(), Value::Byte(i8::from(flag))),
        None => fields.remove(ACCEPT_TEXTURES),
    };
    if entry.hidden || fields.contains_key(HIDDEN) {
        fields.insert(HIDDEN.to_owned(), Value::Byte(i8::from(entry.hidden)));
    }
    Ok(Value::Compound(fields))
}

/// Turns leftover tags into preserved fields, sorted by name for stable output.
fn preserve(fields: HashMap<String, Value>) -> Result<Preserved, ServerListStoreError> {
    let mut preserved = fields
        .into_iter()
        .map(|(name, value)| {
            Ok(PreservedField {
                name,
                kind: tag_id(&value),
                payload: encode_payload(&value)?,
            })
        })
        .collect::<Result<Vec<_>, ServerListStoreError>>()?;
    preserved.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Preserved(preserved))
}

fn restore(field: &PreservedField) -> Result<Value, ServerListStoreError> {
    decode_payload(&field.payload, field.kind)
}

fn encode_payload(value: &Value) -> Result<Vec<u8>, ServerListStoreError> {
    let wrapper = HashMap::from([(PAYLOAD_KEY.to_owned(), value.clone())]);
    fastnbt::to_bytes(&Value::Compound(wrapper))
        .map_err(|error| corrupt(format!("cannot encode a preserved tag: {error}")))
}

fn decode_payload(payload: &[u8], kind: u8) -> Result<Value, ServerListStoreError> {
    let mut wrapper: HashMap<String, Value> = fastnbt::from_bytes(payload)
        .map_err(|error| corrupt(format!("a preserved tag is malformed: {error}")))?;
    wrapper
        .remove(PAYLOAD_KEY)
        .filter(|value| tag_id(value) == kind)
        .ok_or_else(|| corrupt("a preserved tag does not match its kind"))
}

fn tag_id(value: &Value) -> u8 {
    match value {
        Value::Byte(_) => 1,
        Value::Short(_) => 2,
        Value::Int(_) => 3,
        Value::Long(_) => 4,
        Value::Float(_) => 5,
        Value::Double(_) => 6,
        Value::ByteArray(_) => 7,
        Value::String(_) => 8,
        Value::List(_) => 9,
        Value::Compound(_) => TAG_COMPOUND,
        Value::IntArray(_) => 11,
        Value::LongArray(_) => 12,
    }
}

fn corrupt(reason: impl Into<String>) -> ServerListStoreError {
    ServerListStoreError::Corrupt(reason.into())
}
