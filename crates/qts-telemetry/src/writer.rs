// SPDX-License-Identifier: Apache-2.0
//! The telemetry CSV format.
//!
//! A file is UTF-8 text with `\n` line endings:
//!
//! ```text
//! # format = vehicle-telemetry-csv
//! # format_version = 1
//! # generator = <product name and version>
//! # <key> = <value>                (any number of metadata lines)
//! # channel <name> [<unit>] <description>   (one per column, in order)
//! <name>,<name>,...                (header row)
//! <value>,<value>,...              (one row per sample)
//! ```
//!
//! Values are decimal numbers with `.` as the decimal separator. A value
//! that is not finite is written as an empty field.

use std::io::{self, BufRead, Write};

/// Identifier written in the first line of every file. Neutral on purpose:
/// the product name appears only in the `generator` line.
pub const FORMAT_ID: &str = "vehicle-telemetry-csv";

/// Version of the layout described in this module.
pub const FORMAT_VERSION: u32 = 1;

/// One telemetry column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    /// Column name, lower case with underscores, unit suffix included.
    pub name: &'static str,
    /// Unit in plain text, such as `m/s` or `N`. `-` for dimensionless.
    pub unit: &'static str,
    /// One-line description.
    pub description: &'static str,
}

/// Writes telemetry samples as CSV.
#[derive(Debug)]
pub struct TelemetryWriter<W: Write> {
    out: W,
    columns: usize,
    decimation: u32,
    counter: u32,
    rows: u64,
    line: String,
}

impl<W: Write> TelemetryWriter<W> {
    /// Starts a file: writes the metadata, channel list and header row.
    ///
    /// `generator` is the product name and version. `metadata` is written
    /// as `# key = value` lines in the given order. Only every
    /// `decimation`-th call to [`TelemetryWriter::sample`] writes a row.
    pub fn new(
        mut out: W,
        generator: &str,
        metadata: &[(String, String)],
        channels: &[Channel],
        decimation: u32,
    ) -> io::Result<Self> {
        writeln!(out, "# format = {FORMAT_ID}")?;
        writeln!(out, "# format_version = {FORMAT_VERSION}")?;
        writeln!(out, "# generator = {generator}")?;
        for (key, value) in metadata {
            writeln!(out, "# {} = {}", key.trim(), value.replace('\n', " "))?;
        }
        for c in channels {
            writeln!(out, "# channel {} [{}] {}", c.name, c.unit, c.description)?;
        }
        let header: Vec<&str> = channels.iter().map(|c| c.name).collect();
        writeln!(out, "{}", header.join(","))?;
        Ok(Self {
            out,
            columns: channels.len(),
            decimation: decimation.max(1),
            counter: 0,
            rows: 0,
            line: String::new(),
        })
    }

    /// Offers one sample. Returns `true` if a row was written.
    pub fn sample(&mut self, values: &[f64]) -> io::Result<bool> {
        if values.len() != self.columns {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("expected {} values, got {}", self.columns, values.len()),
            ));
        }
        let due = self.counter == 0;
        self.counter = (self.counter + 1) % self.decimation;
        if !due {
            return Ok(false);
        }
        self.line.clear();
        for (i, v) in values.iter().enumerate() {
            if i > 0 {
                self.line.push(',');
            }
            if v.is_finite() {
                use std::fmt::Write as _;
                // Six significant decimals is finer than any channel needs
                // and keeps files compact.
                let _ = write!(self.line, "{v:.6}");
                trim_trailing_zeros(&mut self.line);
            }
        }
        self.line.push('\n');
        self.out.write_all(self.line.as_bytes())?;
        self.rows += 1;
        Ok(true)
    }

    /// Rows written so far.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// Flushes and returns the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Removes trailing zeros (and a trailing decimal point) from the last
/// number in `s`.
fn trim_trailing_zeros(s: &mut String) {
    let start = s.rfind(',').map_or(0, |i| i + 1);
    if !s[start..].contains('.') {
        return;
    }
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if &s[start..] == "-0" {
        s.truncate(start);
        s.push('0');
    }
}

/// A telemetry file read back into memory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryFile {
    /// Metadata lines as `(key, value)`, in file order.
    pub metadata: Vec<(String, String)>,
    /// Column names.
    pub columns: Vec<String>,
    /// Rows of values; an empty field reads as NaN.
    pub rows: Vec<Vec<f64>>,
}

impl TelemetryFile {
    /// Value of a metadata key, if present.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.metadata
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Index of a column by name.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    /// All values of one column.
    pub fn column(&self, name: &str) -> Option<Vec<f64>> {
        let i = self.column_index(name)?;
        Some(self.rows.iter().map(|r| r[i]).collect())
    }
}

/// Reads a file written by [`TelemetryWriter`].
pub fn read_telemetry<R: BufRead>(reader: R) -> io::Result<TelemetryFile> {
    let mut file = TelemetryFile::default();
    let mut header_seen = false;
    for (number, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            let comment = comment.trim();
            if comment.starts_with("channel ") {
                continue;
            }
            if let Some((key, value)) = comment.split_once('=') {
                file.metadata
                    .push((key.trim().to_string(), value.trim().to_string()));
            }
            continue;
        }
        if !header_seen {
            file.columns = line.split(',').map(|s| s.trim().to_string()).collect();
            header_seen = true;
            continue;
        }
        let mut row = Vec::with_capacity(file.columns.len());
        for field in line.split(',') {
            let field = field.trim();
            row.push(if field.is_empty() {
                f64::NAN
            } else {
                field.parse::<f64>().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("line {}: `{field}`: {e}", number + 1),
                    )
                })?
            });
        }
        if row.len() != file.columns.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "line {}: {} fields, header has {}",
                    number + 1,
                    row.len(),
                    file.columns.len()
                ),
            ));
        }
        file.rows.push(row);
    }
    if file.meta("format") != Some(FORMAT_ID) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a telemetry file: missing `# format` line",
        ));
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNELS: [Channel; 3] = [
        Channel {
            name: "time_s",
            unit: "s",
            description: "Simulated time",
        },
        Channel {
            name: "speed_m_s",
            unit: "m/s",
            description: "Speed over the ground",
        },
        Channel {
            name: "gear",
            unit: "-",
            description: "Selected gear",
        },
    ];

    #[test]
    fn written_file_reads_back() {
        let meta = vec![("car".to_string(), "test".to_string())];
        let mut w = TelemetryWriter::new(Vec::new(), "Example 1.0", &meta, &CHANNELS, 2).unwrap();
        assert!(w.sample(&[0.0, 1.5, 1.0]).unwrap());
        assert!(!w.sample(&[0.001, 1.6, 1.0]).unwrap());
        assert!(w.sample(&[0.002, -0.000_000_1, 2.0]).unwrap());
        assert!(!w.sample(&[0.003, 1.8, 2.0]).unwrap());
        assert!(w.sample(&[0.004, f64::NAN, 3.0]).unwrap());
        assert_eq!(w.rows(), 3);
        let bytes = w.finish().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("# format = vehicle-telemetry-csv\n# format_version = 1\n"));
        assert!(text.contains("# channel speed_m_s [m/s] Speed over the ground\n"));
        assert!(text.contains("\ntime_s,speed_m_s,gear\n"));
        assert!(text.contains("\n0,1.5,1\n"));
        assert!(text.contains("\n0.002,0,2\n"), "{text}");
        assert!(text.ends_with("0.004,,3\n"));

        let file = read_telemetry(bytes.as_slice()).unwrap();
        assert_eq!(file.meta("generator"), Some("Example 1.0"));
        assert_eq!(file.meta("car"), Some("test"));
        assert_eq!(file.columns, ["time_s", "speed_m_s", "gear"]);
        assert_eq!(file.rows.len(), 3);
        assert_eq!(file.column("gear").unwrap(), [1.0, 2.0, 3.0]);
        assert!(file.rows[2][1].is_nan());
    }

    #[test]
    fn wrong_column_count_and_foreign_files_are_rejected() {
        let mut w = TelemetryWriter::new(Vec::new(), "x", &[], &CHANNELS, 1).unwrap();
        assert!(w.sample(&[1.0]).is_err());
        assert!(read_telemetry("a,b\n1,2\n".as_bytes()).is_err());
    }
}
