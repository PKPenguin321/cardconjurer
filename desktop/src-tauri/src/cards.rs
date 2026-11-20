//! File-based storage for saved cards.
//!
//! Each saved card is one file in the cards folder, shaped exactly like an
//! entry in the web version's "Download All" export:
//!
//! ```json
//! {"key":"My Card","data":{...card JSON...}}
//! ```
//!
//! File names are derived from the key (a readable part plus a hash of the
//! exact key), so names that differ only in case or contain characters
//! Windows doesn't allow in file names never collide. The real key is always
//! read back from the file itself, never from its name.
//!
//! This module uses only the standard library so it can be unit-tested
//! without pulling in Tauri.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

const EXTENSION: &str = "json";
const KEY_PREFIX: &str = "{\"key\":";
const DATA_SEPARATOR: &str = ",\"data\":";
const READABLE_NAME_MAX_CHARS: usize = 60;

pub struct CardStore {
    dir: PathBuf,
}

impl CardStore {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Keys of all saved cards, sorted.
    pub fn list(&self) -> io::Result<Vec<String>> {
        let mut keys = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) {
                continue;
            }
            match read_key(&path) {
                Ok(Some(key)) => keys.push(key),
                Ok(None) => eprintln!("Skipping unrecognized card file {}", path.display()),
                Err(err) => eprintln!("Couldn't read card file {}: {err}", path.display()),
            }
        }
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    /// The card's JSON (the `data` part), or `None` if no card has this key.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        let path = self.path_for(key);
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        match split_file(&contents) {
            Some((stored_key, data)) if stored_key == key => Ok(Some(data.to_string())),
            Some(_) => Ok(None), // hash collision with a different key: treat as missing
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a valid saved card file", path.display()),
            )),
        }
    }

    /// Saves (or overwrites) a card. `data` must be the card's JSON text.
    pub fn set(&self, key: &str, data: &str) -> io::Result<()> {
        if key.trim().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "card name is empty"));
        }
        let data = data.trim();
        if data.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "card data is empty"));
        }
        let path = self.path_for(key);
        let mut contents = String::with_capacity(data.len() + key.len() + 32);
        contents.push_str(KEY_PREFIX);
        push_json_string(&mut contents, key);
        contents.push_str(DATA_SEPARATOR);
        contents.push_str(data);
        contents.push('}');
        write_atomically(&path, contents.as_bytes())
    }

    /// Deletes a card. Deleting a card that doesn't exist is not an error.
    pub fn remove(&self, key: &str) -> io::Result<()> {
        match fs::remove_file(self.path_for(key)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    /// Deletes every saved card.
    pub fn clear(&self) -> io::Result<()> {
        for key in self.list()? {
            self.remove(&key)?;
        }
        Ok(())
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(file_name_for(key))
    }
}

/// `<readable part>-<16 hex digits of an FNV-1a hash of the exact key>.json`
fn file_name_for(key: &str) -> String {
    let mut readable: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_()[]',!&+=@~".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(READABLE_NAME_MAX_CHARS)
        .collect();
    // Windows drops trailing spaces and dots from file names.
    while readable.ends_with(' ') || readable.ends_with('.') {
        readable.pop();
    }
    let readable = readable.trim_start();
    let readable = if readable.is_empty() { "card" } else { readable };
    format!("{readable}-{:016x}.{EXTENSION}", fnv1a_64(key.as_bytes()))
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Writes to a temporary file first, then renames it over the target, so a
/// crash mid-save can't leave a half-written card behind.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("{EXTENSION}.tmp"));
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Reads just enough of a card file to get its key.
fn read_key(path: &Path) -> io::Result<Option<String>> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    let mut prefix = [0u8; KEY_PREFIX.len()];
    if reader.read_exact(&mut prefix).is_err() || prefix != KEY_PREFIX.as_bytes() {
        return Ok(None);
    }
    // The key is a JSON string; read up to its closing quote.
    let mut raw = Vec::new();
    let mut escaped = false;
    let mut started = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(None);
        }
        let mut consumed = 0;
        let mut done = false;
        for &byte in buf {
            consumed += 1;
            raw.push(byte);
            if !started {
                if byte != b'"' {
                    return Ok(None);
                }
                started = true;
            } else if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                done = true;
                break;
            }
        }
        reader.consume(consumed);
        if done {
            break;
        }
    }
    let raw = String::from_utf8(raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(parse_json_string(&raw).map(|(key, _)| key))
}

/// Splits a card file into its key and the raw `data` JSON.
fn split_file(contents: &str) -> Option<(String, &str)> {
    let rest = contents.trim().strip_prefix(KEY_PREFIX)?;
    let (key, consumed) = parse_json_string(rest)?;
    let data = rest[consumed..].strip_prefix(DATA_SEPARATOR)?;
    let data = data.strip_suffix('}')?;
    Some((key, data.trim()))
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses a JSON string literal at the start of `input`. Returns the decoded
/// string and the number of bytes consumed (including both quotes).
fn parse_json_string(input: &str) -> Option<(String, usize)> {
    let mut chars = input.char_indices();
    if chars.next()?.1 != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, i + 1)),
            '\\' => match chars.next()?.1 {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let high = read_hex4(&mut chars)?;
                    let code = if (0xD800..0xDC00).contains(&high) {
                        // Surrogate pair: expect \uDC00-\uDFFF next.
                        if chars.next()?.1 != '\\' || chars.next()?.1 != 'u' {
                            return None;
                        }
                        let low = read_hex4(&mut chars)?;
                        if !(0xDC00..0xE000).contains(&low) {
                            return None;
                        }
                        0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                    } else {
                        high
                    };
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

fn read_hex4(chars: &mut std::str::CharIndices<'_>) -> Option<u32> {
    let mut value = 0;
    for _ in 0..4 {
        value = value * 16 + chars.next()?.1.to_digit(16)?;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> CardStore {
        let dir = std::env::temp_dir().join(format!("cc-cards-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        CardStore::new(dir).unwrap()
    }

    #[test]
    fn round_trip_and_list() {
        let store = temp_store("roundtrip");
        store.set("Zombie", r#"{"a":1}"#).unwrap();
        store.set("Angel of \"Dawn\"", r#"{"b":[1,2]}"#).unwrap();
        assert_eq!(store.list().unwrap(), vec!["Angel of \"Dawn\"", "Zombie"]);
        assert_eq!(store.get("Zombie").unwrap().as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(store.get("Angel of \"Dawn\"").unwrap().as_deref(), Some(r#"{"b":[1,2]}"#));
        assert_eq!(store.get("Missing").unwrap(), None);
    }

    #[test]
    fn overwrite_remove_clear() {
        let store = temp_store("overwrite");
        store.set("Card", "{\"v\":1}").unwrap();
        store.set("Card", "{\"v\":2}").unwrap();
        assert_eq!(store.list().unwrap(), vec!["Card"]);
        assert_eq!(store.get("Card").unwrap().as_deref(), Some("{\"v\":2}"));
        store.remove("Card").unwrap();
        store.remove("Card").unwrap(); // already gone: fine
        assert!(store.list().unwrap().is_empty());
        store.set("A", "{}").unwrap();
        store.set("B", "{}").unwrap();
        store.clear().unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn names_differing_only_in_case_or_symbols_dont_collide() {
        let store = temp_store("collide");
        for key in ["goblin", "Goblin", "a/b", "a\\b", "a:b", "CON", "x.", "x", "Ünïcødé 🃏"] {
            store.set(key, &format!("{{\"k\":{}}}", key.len())).unwrap();
        }
        let names: std::collections::HashSet<_> = ["goblin", "Goblin", "a/b", "a\\b", "a:b"]
            .iter()
            .map(|k| file_name_for(k).to_lowercase())
            .collect();
        assert_eq!(names.len(), 5, "file names must differ even case-insensitively");
        assert_eq!(store.list().unwrap().len(), 9);
        assert!(store.set("   ", "{}").is_err(), "blank names are rejected");
        assert_eq!(store.get("Ünïcødé 🃏").unwrap().as_deref(), Some(format!("{{\"k\":{}}}", "Ünïcødé 🃏".len()).as_str()));
    }

    #[test]
    fn file_names_are_windows_safe() {
        for key in ["a<b>c", "trailing. . ", "CON", "", "../../etc/passwd"] {
            let name = file_name_for(key);
            assert!(!name.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']), "{name}");
            assert!(!name.starts_with("CON."), "{name}");
        }
    }

    #[test]
    fn json_string_escaping_round_trips() {
        for value in ["plain", "q\"uote", "back\\slash", "line\nbreak\ttab", "\u{1}", "emoji 🃏"] {
            let mut encoded = String::new();
            push_json_string(&mut encoded, value);
            assert_eq!(parse_json_string(&encoded), Some((value.to_string(), encoded.len())));
        }
        assert_eq!(parse_json_string(r#""🃏""#).unwrap().0, "🃏");
    }

    #[test]
    fn files_match_web_export_entries() {
        let store = temp_store("format");
        store.set("My Card", "{\"x\":1}").unwrap();
        let text = fs::read_to_string(store.path_for("My Card")).unwrap();
        assert_eq!(text, r#"{"key":"My Card","data":{"x":1}}"#);
    }
}
