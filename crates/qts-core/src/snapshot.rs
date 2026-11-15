// SPDX-License-Identifier: Apache-2.0
//! State snapshot, restore and hashing.
//!
//! Every stateful simulation type implements [`Snapshot`]. The byte layout is
//! explicit (little-endian, fixed field order) and the per-tick state hash is
//! computed over the same byte stream, so a field cannot be restored but
//! missing from the hash, or the reverse.

use crate::math::{Quat, Vec3};
use std::fmt;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 64-bit FNV-1a hash of a byte slice.
///
/// Used for desync detection only. It is not cryptographic and gives no
/// protection against deliberate tampering.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// Error returned when a snapshot cannot be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    /// The byte stream ended before all state was read.
    UnexpectedEnd,
    /// Bytes were left over after all state was read.
    TrailingBytes(usize),
    /// A value was outside the range the reader accepts.
    Invalid(&'static str),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::UnexpectedEnd => write!(f, "snapshot ended unexpectedly"),
            SnapshotError::TrailingBytes(n) => write!(f, "snapshot has {n} unread trailing bytes"),
            SnapshotError::Invalid(what) => write!(f, "snapshot holds an invalid value: {what}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Sink for state bytes. Always maintains a running FNV-1a hash; keeps the
/// bytes too when created with [`StateWriter::recording`].
#[derive(Clone, Debug)]
pub struct StateWriter {
    hash: u64,
    len: usize,
    buf: Option<Vec<u8>>,
}

impl StateWriter {
    /// A writer that keeps the bytes (for snapshots).
    pub fn recording() -> Self {
        Self {
            hash: FNV_OFFSET,
            len: 0,
            buf: Some(Vec::new()),
        }
    }

    /// A writer that only hashes (for the per-tick state hash, no allocation).
    pub fn hashing() -> Self {
        Self {
            hash: FNV_OFFSET,
            len: 0,
            buf: None,
        }
    }

    /// Hash of everything written so far.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// Number of bytes written so far.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Consumes the writer and returns the recorded bytes (empty for a
    /// hashing-only writer).
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf.unwrap_or_default()
    }

    /// Writes raw bytes.
    pub fn write_raw(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.hash ^= u64::from(b);
            self.hash = self.hash.wrapping_mul(FNV_PRIME);
        }
        self.len += bytes.len();
        if let Some(buf) = self.buf.as_mut() {
            buf.extend_from_slice(bytes);
        }
    }

    /// Writes a `u8`.
    pub fn write_u8(&mut self, v: u8) {
        self.write_raw(&[v]);
    }
    /// Writes a `bool` as one byte.
    pub fn write_bool(&mut self, v: bool) {
        self.write_u8(u8::from(v));
    }
    /// Writes an `i8`.
    pub fn write_i8(&mut self, v: i8) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes a `u16`.
    pub fn write_u16(&mut self, v: u16) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes an `i16`.
    pub fn write_i16(&mut self, v: i16) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes a `u32`.
    pub fn write_u32(&mut self, v: u32) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes an `i32`.
    pub fn write_i32(&mut self, v: i32) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes a `u64`.
    pub fn write_u64(&mut self, v: u64) {
        self.write_raw(&v.to_le_bytes());
    }
    /// Writes an `f64` as its IEEE-754 bit pattern.
    pub fn write_f64(&mut self, v: f64) {
        self.write_raw(&v.to_bits().to_le_bytes());
    }
    /// Writes each element of a slice of `f64`, without a length prefix.
    pub fn write_f64s(&mut self, v: &[f64]) {
        for &x in v {
            self.write_f64(x);
        }
    }
    /// Writes a vector.
    pub fn write_vec3(&mut self, v: Vec3) {
        self.write_f64(v.x);
        self.write_f64(v.y);
        self.write_f64(v.z);
    }
    /// Writes a quaternion.
    pub fn write_quat(&mut self, q: Quat) {
        self.write_f64(q.w);
        self.write_f64(q.x);
        self.write_f64(q.y);
        self.write_f64(q.z);
    }
    /// Writes a length-prefixed byte string.
    pub fn write_bytes(&mut self, v: &[u8]) {
        self.write_u32(v.len() as u32);
        self.write_raw(v);
    }
    /// Writes a length-prefixed UTF-8 string.
    pub fn write_str(&mut self, v: &str) {
        self.write_bytes(v.as_bytes());
    }
}

/// Cursor over snapshot bytes.
#[derive(Clone, Debug)]
pub struct StateReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> StateReader<'a> {
    /// Starts reading at the beginning of `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes not yet read.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Fails if any bytes are left unread.
    pub fn finish(&self) -> Result<(), SnapshotError> {
        match self.remaining() {
            0 => Ok(()),
            n => Err(SnapshotError::TrailingBytes(n)),
        }
    }

    /// Reads `n` raw bytes.
    pub fn read_raw(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        if self.remaining() < n {
            return Err(SnapshotError::UnexpectedEnd);
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], SnapshotError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.read_raw(N)?);
        Ok(out)
    }

    /// Reads a `u8`.
    pub fn read_u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.read_array::<1>()?[0])
    }
    /// Reads a `bool`; any byte other than 0 or 1 is an error.
    pub fn read_bool(&mut self) -> Result<bool, SnapshotError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(SnapshotError::Invalid("bool")),
        }
    }
    /// Reads an `i8`.
    pub fn read_i8(&mut self) -> Result<i8, SnapshotError> {
        Ok(i8::from_le_bytes(self.read_array()?))
    }
    /// Reads a `u16`.
    pub fn read_u16(&mut self) -> Result<u16, SnapshotError> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }
    /// Reads an `i16`.
    pub fn read_i16(&mut self) -> Result<i16, SnapshotError> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }
    /// Reads a `u32`.
    pub fn read_u32(&mut self) -> Result<u32, SnapshotError> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }
    /// Reads an `i32`.
    pub fn read_i32(&mut self) -> Result<i32, SnapshotError> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }
    /// Reads a `u64`.
    pub fn read_u64(&mut self) -> Result<u64, SnapshotError> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }
    /// Reads an `f64`.
    pub fn read_f64(&mut self) -> Result<f64, SnapshotError> {
        Ok(f64::from_bits(u64::from_le_bytes(self.read_array()?)))
    }
    /// Fills a slice of `f64`.
    pub fn read_f64s(&mut self, out: &mut [f64]) -> Result<(), SnapshotError> {
        for x in out {
            *x = self.read_f64()?;
        }
        Ok(())
    }
    /// Reads a vector.
    pub fn read_vec3(&mut self) -> Result<Vec3, SnapshotError> {
        Ok(Vec3::new(
            self.read_f64()?,
            self.read_f64()?,
            self.read_f64()?,
        ))
    }
    /// Reads a quaternion.
    pub fn read_quat(&mut self) -> Result<Quat, SnapshotError> {
        Ok(Quat {
            w: self.read_f64()?,
            x: self.read_f64()?,
            y: self.read_f64()?,
            z: self.read_f64()?,
        })
    }
    /// Reads a length-prefixed byte string.
    pub fn read_bytes(&mut self) -> Result<&'a [u8], SnapshotError> {
        let n = self.read_u32()? as usize;
        self.read_raw(n)
    }
    /// Reads a length-prefixed UTF-8 string.
    pub fn read_string(&mut self) -> Result<String, SnapshotError> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SnapshotError::Invalid("utf-8 string"))
    }
}

/// A type whose complete dynamic state can be saved and restored.
///
/// `save` and `load` must visit the same fields in the same order. Parameters
/// that never change during a run are not state and are not written.
pub trait Snapshot {
    /// Writes all dynamic state.
    fn save(&self, w: &mut StateWriter);
    /// Restores all dynamic state written by [`Snapshot::save`].
    fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError>;
}

/// Serialises the state of `s` to bytes.
pub fn snapshot_bytes<S: Snapshot + ?Sized>(s: &S) -> Vec<u8> {
    let mut w = StateWriter::recording();
    s.save(&mut w);
    w.into_bytes()
}

/// Restores `s` from bytes produced by [`snapshot_bytes`], rejecting
/// truncated or over-long input.
pub fn restore_bytes<S: Snapshot + ?Sized>(s: &mut S, bytes: &[u8]) -> Result<(), SnapshotError> {
    let mut r = StateReader::new(bytes);
    s.load(&mut r)?;
    r.finish()
}

/// Hash of the state of `s`, identical to `fnv1a64(&snapshot_bytes(s))` but
/// without allocating.
pub fn state_hash<S: Snapshot + ?Sized>(s: &S) -> u64 {
    let mut w = StateWriter::hashing();
    s.save(&mut w);
    w.hash()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_published_vectors() {
        // Reference values from the FNV specification test suite.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[derive(Default, PartialEq, Debug)]
    struct Sample {
        a: f64,
        b: u32,
        c: bool,
        v: Vec3,
        name: String,
    }

    impl Snapshot for Sample {
        fn save(&self, w: &mut StateWriter) {
            w.write_f64(self.a);
            w.write_u32(self.b);
            w.write_bool(self.c);
            w.write_vec3(self.v);
            w.write_str(&self.name);
        }
        fn load(&mut self, r: &mut StateReader<'_>) -> Result<(), SnapshotError> {
            self.a = r.read_f64()?;
            self.b = r.read_u32()?;
            self.c = r.read_bool()?;
            self.v = r.read_vec3()?;
            self.name = r.read_string()?;
            Ok(())
        }
    }

    #[test]
    fn round_trip_restores_every_field() {
        let s = Sample {
            a: -0.0,
            b: 7,
            c: true,
            v: Vec3::new(1.0, f64::MIN_POSITIVE, -3.5),
            name: "abc".into(),
        };
        let bytes = snapshot_bytes(&s);
        let mut t = Sample::default();
        restore_bytes(&mut t, &bytes).unwrap();
        assert_eq!(s, t);
        // -0.0 == 0.0 under PartialEq, so check the bit pattern survived too.
        assert_eq!(t.a.to_bits(), (-0.0f64).to_bits());
    }

    #[test]
    fn hash_only_writer_agrees_with_recorded_bytes() {
        let s = Sample {
            a: 1.5,
            b: 9,
            c: false,
            v: Vec3::X,
            name: "x".into(),
        };
        assert_eq!(state_hash(&s), fnv1a64(&snapshot_bytes(&s)));
    }

    #[test]
    fn truncated_and_overlong_input_is_rejected() {
        let s = Sample::default();
        let mut bytes = snapshot_bytes(&s);
        let mut t = Sample::default();
        assert_eq!(
            restore_bytes(&mut t, &bytes[..bytes.len() - 1]),
            Err(SnapshotError::UnexpectedEnd)
        );
        bytes.push(0);
        assert_eq!(
            restore_bytes(&mut t, &bytes),
            Err(SnapshotError::TrailingBytes(1))
        );
    }
}
