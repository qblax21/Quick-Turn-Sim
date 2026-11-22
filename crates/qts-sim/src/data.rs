// SPDX-License-Identifier: Apache-2.0
//! Loading of data files from the top-level `data/` directory.
//!
//! Car, tire, surface and force-feedback files hold physical parameters as
//! `{ value, source, confidence }` tables. [`flatten`] validates each one,
//! records its provenance, and replaces the table with its value so the
//! model structs can be plain numbers. A bare number in such a file is an
//! error: no parameter may exist without a stated origin.
//!
//! Track and validation files describe original content and targets, not
//! physical parameters of a car, and are parsed directly.

use crate::track::TrackDef;
use qts_core::provenance::{Confidence, Provenance};
use qts_core::snapshot::fnv1a64;
use qts_ffb::FfbConfig;
use qts_telemetry::import::ImportProfile;
use qts_tire::{SurfaceParams, TireParams};
use qts_vehicle::CarParams;
use serde::de::DeserializeOwned;
use std::fmt;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the data directory location. The
/// name is deliberately neutral (see `DECISIONS.md`, D-014).
pub const DATA_DIR_ENV: &str = "SIM_DATA_DIR";

/// Error from locating, reading or interpreting a data file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataError {
    /// The data directory could not be found.
    NoDataDir(String),
    /// A file could not be read.
    Io {
        /// File that failed.
        path: PathBuf,
        /// Operating system message.
        message: String,
    },
    /// A file is not valid TOML or does not match its schema.
    Parse {
        /// File that failed.
        path: PathBuf,
        /// Parser message.
        message: String,
    },
    /// A parameter is missing its source or confidence, or they are
    /// malformed.
    Provenance {
        /// File that failed.
        path: PathBuf,
        /// What is wrong and where.
        message: String,
    },
    /// The file parsed but its values are outside the accepted ranges or
    /// refer to something that does not exist.
    Invalid {
        /// File that failed.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataError::NoDataDir(m) => write!(f, "data directory not found: {m}"),
            DataError::Io { path, message } => write!(f, "{}: {message}", path.display()),
            DataError::Parse { path, message } => {
                write!(f, "{}: parse error: {message}", path.display())
            }
            DataError::Provenance { path, message } => {
                write!(f, "{}: provenance error: {message}", path.display())
            }
            DataError::Invalid { path, message } => {
                write!(f, "{}: invalid data: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for DataError {}

/// A parsed data file together with where its numbers came from.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
    /// The parsed parameters.
    pub value: T,
    /// Provenance of every parameter in the file (and its base files).
    pub provenance: Vec<Provenance>,
    /// Hash of the file contents with line endings normalised.
    pub content_hash: u64,
}

/// Replaces every `{ value, source, confidence }` table in `value` with its
/// `value`, collecting provenance records. Fails on a bare number or a
/// malformed parameter table. `path` is the dotted location used in
/// messages.
pub fn flatten(
    value: toml::Value,
    path: &str,
    out: &mut Vec<Provenance>,
) -> Result<toml::Value, String> {
    use toml::Value;
    match value {
        Value::Table(table) => {
            if table.contains_key("value") {
                for key in table.keys() {
                    if !matches!(key.as_str(), "value" | "source" | "confidence" | "note") {
                        return Err(format!("{path}: unexpected key `{key}` in a parameter table"));
                    }
                }
                let source = match table.get("source") {
                    Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
                    Some(_) => return Err(format!("{path}: `source` must be a non-empty string")),
                    None => return Err(format!("{path}: parameter has no `source`")),
                };
                let confidence = match table.get("confidence") {
                    Some(Value::String(s)) => Confidence::parse(s).ok_or_else(|| {
                        format!(
                            "{path}: confidence `{s}` is not one of placeholder, estimated, measured"
                        )
                    })?,
                    Some(_) => return Err(format!("{path}: `confidence` must be a string")),
                    None => return Err(format!("{path}: parameter has no `confidence`")),
                };
                let inner = table.get("value").cloned().unwrap_or(Value::Boolean(false));
                match &inner {
                    Value::Integer(_) | Value::Float(_) => {}
                    Value::Array(items)
                        if items
                            .iter()
                            .all(|v| matches!(v, Value::Integer(_) | Value::Float(_))) => {}
                    _ => {
                        return Err(format!(
                            "{path}: `value` must be a number or an array of numbers"
                        ))
                    }
                }
                out.push(Provenance {
                    path: path.to_string(),
                    source,
                    confidence,
                });
                Ok(inner)
            } else {
                let mut flat = toml::Table::new();
                for (key, child) in table {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    flat.insert(key, flatten(child, &child_path, out)?);
                }
                Ok(Value::Table(flat))
            }
        }
        Value::Integer(_) | Value::Float(_) => Err(format!(
            "{path}: bare number; write it as {{ value = ..., source = \"...\", confidence = \"...\" }}"
        )),
        Value::Array(items) => {
            let mut flat = Vec::with_capacity(items.len());
            for (i, item) in items.into_iter().enumerate() {
                flat.push(flatten(item, &format!("{path}[{i}]"), out)?);
            }
            Ok(Value::Array(flat))
        }
        other => Ok(other),
    }
}

/// Recursively overlays `over` on `base`. Tables merge key by key; a
/// parameter table (one with a `value` key) and every other value replace
/// the base entry whole.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(o))
                if !b.contains_key("value") && !o.contains_key("value") =>
            {
                merge(b, o);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

fn normalised_hash(text: &str) -> u64 {
    let bytes: Vec<u8> = text.bytes().filter(|b| *b != b'\r').collect();
    fnv1a64(&bytes)
}

/// Handle to the `data/` directory.
#[derive(Clone, Debug)]
pub struct DataRoot {
    root: PathBuf,
}

impl DataRoot {
    /// Uses `root` as the data directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Finds the data directory: the [`DATA_DIR_ENV`] variable if set,
    /// otherwise the first `data` directory containing `cars` found by
    /// walking up from the current directory and then from the executable.
    pub fn discover() -> Result<Self, DataError> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            let root = PathBuf::from(dir);
            if root.join("cars").is_dir() {
                return Ok(Self { root });
            }
            return Err(DataError::NoDataDir(format!(
                "{DATA_DIR_ENV} points at {} which has no `cars` directory",
                root.display()
            )));
        }
        let mut starts = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            starts.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                starts.push(dir.to_path_buf());
            }
        }
        for start in starts {
            let mut dir: Option<&Path> = Some(start.as_path());
            while let Some(d) = dir {
                let candidate = d.join("data");
                if candidate.join("cars").is_dir() {
                    return Ok(Self { root: candidate });
                }
                dir = d.parent();
            }
        }
        Err(DataError::NoDataDir(format!(
            "no `data/cars` directory above the current directory or the executable; set {DATA_DIR_ENV}"
        )))
    }

    /// The directory this handle points at.
    pub fn path(&self) -> &Path {
        &self.root
    }

    fn file(&self, kind: &str, id: &str) -> PathBuf {
        self.root.join(kind).join(format!("{id}.toml"))
    }

    fn read(&self, path: &Path) -> Result<String, DataError> {
        std::fs::read_to_string(path).map_err(|e| DataError::Io {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    fn check_id(path: &Path, kind: &str, id: &str) -> Result<(), DataError> {
        let ok = !id.is_empty()
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if ok {
            Ok(())
        } else {
            Err(DataError::Invalid {
                path: path.to_path_buf(),
                message: format!("`{id}` is not a valid {kind} identifier"),
            })
        }
    }

    /// Identifiers of all files of one kind (`cars`, `tires`, `surfaces`,
    /// `tracks`, `ffb`, `validation`, `import`), sorted.
    pub fn list(&self, kind: &str) -> Result<Vec<String>, DataError> {
        let dir = self.root.join(kind);
        let entries = std::fs::read_dir(&dir).map_err(|e| DataError::Io {
            path: dir.clone(),
            message: e.to_string(),
        })?;
        let mut ids = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Reads a file as a TOML table, following `inherits` chains. Returns
    /// the merged table and a hash covering every file read.
    fn read_table(
        &self,
        kind: &str,
        id: &str,
        depth: u32,
    ) -> Result<(toml::Table, u64), DataError> {
        let path = self.file(kind, id);
        Self::check_id(&path, kind, id)?;
        let text = self.read(&path)?;
        let mut hash = normalised_hash(&text);
        let mut table: toml::Table = toml::from_str(&text).map_err(|e| DataError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        if let Some(base) = table.remove("inherits") {
            let base_id = base.as_str().ok_or_else(|| DataError::Parse {
                path: path.clone(),
                message: "`inherits` must be a string".into(),
            })?;
            if depth >= 4 {
                return Err(DataError::Invalid {
                    path,
                    message: "`inherits` chain is too deep or circular".into(),
                });
            }
            let (mut merged, base_hash) = self.read_table(kind, base_id, depth + 1)?;
            merge(&mut merged, table);
            table = merged;
            hash = fnv1a64(&[base_hash.to_le_bytes(), hash.to_le_bytes()].concat());
        }
        Ok((table, hash))
    }

    /// Loads a file whose numbers must all carry provenance.
    pub fn load_parameters<T: DeserializeOwned>(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<Loaded<T>, DataError> {
        let path = self.file(kind, id);
        let (table, content_hash) = self.read_table(kind, id, 0)?;
        let mut provenance = Vec::new();
        let flat = flatten(toml::Value::Table(table), "", &mut provenance).map_err(|message| {
            DataError::Provenance {
                path: path.clone(),
                message,
            }
        })?;
        let value: T = flat
            .try_into()
            .map_err(|e: toml::de::Error| DataError::Parse {
                path: path.clone(),
                message: e.to_string(),
            })?;
        Ok(Loaded {
            value,
            provenance,
            content_hash,
        })
    }

    /// Loads a file that is parsed directly, without provenance tables.
    pub fn load_plain<T: DeserializeOwned>(
        &self,
        kind: &str,
        id: &str,
    ) -> Result<(T, u64), DataError> {
        let path = self.file(kind, id);
        Self::check_id(&path, kind, id)?;
        let text = self.read(&path)?;
        let value: T = toml::from_str(&text).map_err(|e| DataError::Parse {
            path: path.clone(),
            message: e.to_string(),
        })?;
        Ok((value, normalised_hash(&text)))
    }

    /// Loads and validates a car.
    pub fn load_car(&self, id: &str) -> Result<Loaded<CarParams>, DataError> {
        let loaded: Loaded<CarParams> = self.load_parameters("cars", id)?;
        let path = self.file("cars", id);
        if loaded.value.id != id {
            return Err(DataError::Invalid {
                path,
                message: format!("`id` is `{}` but the file is `{id}`", loaded.value.id),
            });
        }
        loaded
            .value
            .validate()
            .map_err(|message| DataError::Invalid { path, message })?;
        Ok(loaded)
    }

    /// Loads and validates a tire.
    pub fn load_tire(&self, id: &str) -> Result<Loaded<TireParams>, DataError> {
        let loaded: Loaded<TireParams> = self.load_parameters("tires", id)?;
        let path = self.file("tires", id);
        if loaded.value.id != id {
            return Err(DataError::Invalid {
                path,
                message: format!("`id` is `{}` but the file is `{id}`", loaded.value.id),
            });
        }
        loaded
            .value
            .validate()
            .map_err(|message| DataError::Invalid { path, message })?;
        Ok(loaded)
    }

    /// Loads and validates a surface.
    pub fn load_surface(&self, id: &str) -> Result<Loaded<SurfaceParams>, DataError> {
        let loaded: Loaded<SurfaceParams> = self.load_parameters("surfaces", id)?;
        let path = self.file("surfaces", id);
        if loaded.value.id != id {
            return Err(DataError::Invalid {
                path,
                message: format!("`id` is `{}` but the file is `{id}`", loaded.value.id),
            });
        }
        loaded
            .value
            .validate()
            .map_err(|message| DataError::Invalid { path, message })?;
        Ok(loaded)
    }

    /// Loads every surface, in identifier order. The position in the
    /// returned list is the surface index used throughout a session.
    pub fn load_all_surfaces(&self) -> Result<Vec<Loaded<SurfaceParams>>, DataError> {
        self.list("surfaces")?
            .iter()
            .map(|id| self.load_surface(id))
            .collect()
    }

    /// Loads and validates a force-feedback preset.
    pub fn load_ffb(&self, id: &str) -> Result<Loaded<FfbConfig>, DataError> {
        let loaded: Loaded<FfbConfig> = self.load_parameters("ffb", id)?;
        let path = self.file("ffb", id);
        if loaded.value.id != id {
            return Err(DataError::Invalid {
                path,
                message: format!("`id` is `{}` but the file is `{id}`", loaded.value.id),
            });
        }
        loaded
            .value
            .validate()
            .map_err(|message| DataError::Invalid { path, message })?;
        Ok(loaded)
    }

    /// Loads a wheel-base model from `data/wheels`.
    pub fn load_wheel(&self, id: &str) -> Result<Loaded<qts_ffb::WheelModel>, DataError> {
        let loaded: Loaded<qts_ffb::WheelModel> = self.load_parameters("wheels", id)?;
        let path = self.file("wheels", id);
        if loaded.value.id != id {
            return Err(DataError::Invalid {
                path,
                message: format!("`id` is `{}` but the file is `{id}`", loaded.value.id),
            });
        }
        loaded
            .value
            .validate()
            .map_err(|message| DataError::Invalid { path, message })?;
        Ok(loaded)
    }

    /// Loads a column-mapping profile for the telemetry importer.
    pub fn load_import_profile(&self, id: &str) -> Result<ImportProfile, DataError> {
        Ok(self.load_plain::<ImportProfile>("import", id)?.0)
    }

    /// Loads a track definition.
    pub fn load_track(&self, id: &str) -> Result<(TrackDef, u64), DataError> {
        let (def, hash): (TrackDef, u64) = self.load_plain("tracks", id)?;
        if def.id != id {
            return Err(DataError::Invalid {
                path: self.file("tracks", id),
                message: format!("`id` is `{}` but the file is `{id}`", def.id),
            });
        }
        Ok((def, hash))
    }
}

/// Renders a provenance list as a text report, least trusted first.
pub fn provenance_report(title: &str, items: &[Provenance]) -> String {
    let mut sorted: Vec<&Provenance> = items.iter().collect();
    sorted.sort_by(|a, b| a.confidence.cmp(&b.confidence).then(a.path.cmp(&b.path)));
    let count = |c: Confidence| items.iter().filter(|p| p.confidence == c).count();
    let mut out = format!(
        "{title}: {} parameters ({} placeholder, {} estimated, {} measured)\n",
        items.len(),
        count(Confidence::Placeholder),
        count(Confidence::Estimated),
        count(Confidence::Measured)
    );
    for p in sorted {
        out.push_str(&format!(
            "  [{:<11}] {:<52} {}\n",
            p.confidence.as_str(),
            p.path,
            p.source
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> toml::Value {
        toml::Value::Table(toml::from_str(text).unwrap())
    }

    #[test]
    fn flatten_strips_parameter_tables_and_records_provenance() {
        let v = parse(
            r#"
            name = "x"
            enabled = true
            [body]
            mass_kg = { value = 1200.0, source = "guess", confidence = "placeholder" }
            ratios = { value = [3.0, 2], source = "invented", confidence = "estimated" }
            "#,
        );
        let mut prov = Vec::new();
        let flat = flatten(v, "", &mut prov).unwrap();
        assert_eq!(flat["body"]["mass_kg"].as_float(), Some(1200.0));
        assert_eq!(flat["body"]["ratios"].as_array().unwrap().len(), 2);
        assert_eq!(flat["name"].as_str(), Some("x"));
        assert_eq!(prov.len(), 2);
        assert_eq!(prov[0].path, "body.mass_kg");
        assert_eq!(prov[0].confidence, Confidence::Placeholder);
        assert_eq!(prov[1].confidence, Confidence::Estimated);
    }

    #[test]
    fn bare_numbers_and_incomplete_parameters_are_rejected() {
        let mut prov = Vec::new();
        let bare = flatten(parse("mass_kg = 1200.0"), "", &mut prov).unwrap_err();
        assert!(
            bare.contains("mass_kg") && bare.contains("bare number"),
            "{bare}"
        );
        let no_source = flatten(
            parse(r#"m = { value = 1.0, confidence = "measured" }"#),
            "",
            &mut prov,
        )
        .unwrap_err();
        assert!(no_source.contains("no `source`"), "{no_source}");
        let no_conf =
            flatten(parse(r#"m = { value = 1.0, source = "s" }"#), "", &mut prov).unwrap_err();
        assert!(no_conf.contains("no `confidence`"), "{no_conf}");
        let bad_conf = flatten(
            parse(r#"m = { value = 1.0, source = "s", confidence = "certain" }"#),
            "",
            &mut prov,
        )
        .unwrap_err();
        assert!(bad_conf.contains("certain"), "{bad_conf}");
        let extra = flatten(
            parse(r#"m = { value = 1.0, source = "s", confidence = "measured", unit = "kg" }"#),
            "",
            &mut prov,
        )
        .unwrap_err();
        assert!(extra.contains("unit"), "{extra}");
        let text = flatten(
            parse(r#"m = { value = "big", source = "s", confidence = "measured" }"#),
            "",
            &mut prov,
        )
        .unwrap_err();
        assert!(text.contains("number"), "{text}");
    }

    #[test]
    fn merge_overrides_leaves_and_keeps_the_rest() {
        let mut base: toml::Table = toml::from_str(
            r#"
            id = "a"
            [d]
            kind = "open"
            p = { value = 1.0, source = "base", confidence = "placeholder" }
            q = { value = 2.0, source = "base", confidence = "placeholder" }
            "#,
        )
        .unwrap();
        let over: toml::Table = toml::from_str(
            r#"
            id = "b"
            [d]
            kind = "limited_slip"
            q = { value = 5.0, source = "variant", confidence = "estimated" }
            "#,
        )
        .unwrap();
        merge(&mut base, over);
        assert_eq!(base["id"].as_str(), Some("b"));
        assert_eq!(base["d"]["kind"].as_str(), Some("limited_slip"));
        assert_eq!(base["d"]["p"]["value"].as_float(), Some(1.0));
        assert_eq!(base["d"]["q"]["value"].as_float(), Some(5.0));
        assert_eq!(base["d"]["q"]["source"].as_str(), Some("variant"));
    }

    #[test]
    fn line_endings_do_not_change_the_content_hash() {
        assert_eq!(
            normalised_hash("a = 1\nb = 2\n"),
            normalised_hash("a = 1\r\nb = 2\r\n")
        );
        assert_ne!(normalised_hash("a = 1\n"), normalised_hash("a = 2\n"));
    }
}
