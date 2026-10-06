//! zebrad.toml editing that keeps the user's comments and layout (toml_edit).

use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, Table, Value};

pub struct ConfigFile {
    pub path: PathBuf,
    pub doc: DocumentMut,
    pub dirty: bool,
    loaded_text: String,
}

pub struct Entry {
    pub path: Vec<String>,
    pub value: String,
    pub editable: bool,
}

impl Entry {
    pub fn key(&self) -> String {
        self.path.join(".")
    }
}

impl ConfigFile {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = text.parse::<DocumentMut>().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self { path, doc, dirty: false, loaded_text: text })
    }

    pub fn reload(&mut self) -> Result<(), String> {
        *self = Self::load(self.path.clone())?;
        Ok(())
    }

    pub fn changed_on_disk(&self) -> bool {
        std::fs::read_to_string(&self.path).map(|t| t != self.loaded_text).unwrap_or(true)
    }

    /// Writes atomically and keeps a timestamped backup of the previous file next to it.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let text = self.doc.to_string();
        text.parse::<DocumentMut>().map_err(|e| format!("refusing to write invalid TOML: {e}"))?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let backup = PathBuf::from(format!("{}.bak-{stamp}", self.path.display()));
        std::fs::copy(&self.path, &backup).map_err(|e| format!("backup failed: {e}"))?;
        let tmp = PathBuf::from(format!("{}.tui-tmp", self.path.display()));
        std::fs::write(&tmp, &text).map_err(|e| format!("write failed: {e}"))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("write failed: {e}"))?;
        self.loaded_text = text;
        self.dirty = false;
        Ok(backup)
    }

    pub fn entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        flatten(self.doc.as_table(), &mut Vec::new(), &mut out);
        out
    }

    pub fn get(&self, path: &[&str]) -> Option<&Value> {
        let mut item = self.doc.as_item();
        for seg in path {
            item = item.get(seg)?;
        }
        item.as_value()
    }

    pub fn get_str(&self, path: &[&str]) -> Option<String> {
        self.get(path).and_then(|v| v.as_str()).map(String::from)
    }

    pub fn get_int(&self, path: &[&str]) -> Option<i64> {
        self.get(path).and_then(Value::as_integer)
    }

    pub fn get_bool(&self, path: &[&str]) -> Option<bool> {
        self.get(path).and_then(Value::as_bool)
    }

    pub fn set(&mut self, path: &[String], mut value: Value) -> Result<(), String> {
        let (key, sections) = path.split_last().ok_or("empty key")?;
        let mut table: &mut Table = self.doc.as_table_mut();
        for seg in sections {
            table = table
                .entry(seg)
                .or_insert(Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| format!("`{seg}` is not a table"))?;
        }
        match table.get_mut(key) {
            Some(Item::Value(old)) => {
                *value.decor_mut() = old.decor().clone();
                *old = value;
            }
            Some(_) => return Err(format!("`{key}` is a table, not a value")),
            None => {
                table.insert(key, Item::Value(value));
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub fn remove(&mut self, path: &[String]) -> Result<(), String> {
        let (key, sections) = path.split_last().ok_or("empty key")?;
        let mut table: &mut Table = self.doc.as_table_mut();
        for seg in sections {
            table = table.get_mut(seg).and_then(Item::as_table_mut).ok_or("no such key")?;
        }
        table.remove(key).ok_or("no such key")?;
        self.dirty = true;
        Ok(())
    }
}

fn flatten(table: &Table, prefix: &mut Vec<String>, out: &mut Vec<Entry>) {
    for (key, item) in table.iter() {
        prefix.push(key.to_string());
        match item {
            Item::Table(t) => flatten(t, prefix, out),
            Item::Value(v) => out.push(Entry {
                path: prefix.clone(),
                value: v.to_string().trim().to_string(),
                editable: true,
            }),
            Item::ArrayOfTables(a) => out.push(Entry {
                path: prefix.clone(),
                value: format!("[[{}]] x{} (edit in $EDITOR)", prefix.join("."), a.len()),
                editable: false,
            }),
            Item::None => {}
        }
        prefix.pop();
    }
}

/// Parses what the user typed, keeping the type of the value it replaces.
pub fn parse_value(input: &str, like: Option<&Value>) -> Result<Value, String> {
    let s = input.trim();
    match like {
        Some(Value::String(_)) => {
            let unquoted = s.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(s);
            Ok(Value::from(unquoted.to_string()))
        }
        Some(Value::Boolean(_)) => s.parse::<bool>().map(Value::from).map_err(|_| "expected true or false".into()),
        Some(Value::Integer(_)) => s.parse::<i64>().map(Value::from).map_err(|_| "expected a whole number".into()),
        Some(Value::Float(_)) => s.parse::<f64>().map(Value::from).map_err(|_| "expected a number".into()),
        _ => Ok(s.parse::<Value>().unwrap_or_else(|_| Value::from(s.to_string()))),
    }
}

/// The text to prefill an edit box with: strings without their quotes, everything else as TOML.
pub fn edit_text(v: &Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string().trim().to_string(),
    }
}
