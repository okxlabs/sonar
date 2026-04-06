//! Shared binary reader for parsing Solana account and instruction data.
//!
//! Provides a lightweight cursor over a byte slice with little-endian
//! decoding helpers commonly needed when reading on-chain structures.

use anyhow::{Context, Result, bail};
use solana_pubkey::Pubkey;

use super::instruction::ParsedInstruction;

/// A zero-copy cursor over a byte slice with position tracking.
pub(crate) struct BinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[allow(dead_code)]
impl<'a> BinaryReader<'a> {
    /// Wrap a byte slice with an initial offset of 0.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    /// Returns `true` if there are bytes remaining to read.
    pub fn has_remaining(&self) -> bool {
        self.offset < self.bytes.len()
    }

    /// Returns the number of bytes remaining.
    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    /// Returns the current read position.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Read exactly `len` bytes and advance the offset.
    pub fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.offset + len > self.bytes.len() {
            bail!(
                "data too short at offset {}, need {} bytes but only {} remain",
                self.offset,
                len,
                self.remaining()
            );
        }
        let start = self.offset;
        self.offset += len;
        Ok(&self.bytes[start..start + len])
    }

    /// Read a single byte.
    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact(1)?[0])
    }

    /// Read a single byte as a boolean (0=false, 1=true).
    pub fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => bail!("invalid bool discriminant {v}"),
        }
    }

    /// Read a little-endian `u16`.
    pub fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        buf.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(buf))
    }

    /// Read a little-endian `u32`.
    pub fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    /// Read a little-endian `u64`.
    pub fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.read_exact(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    /// Read 32 bytes as a `Pubkey`.
    pub fn read_pubkey(&mut self) -> Result<Pubkey> {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(self.read_exact(32)?);
        Ok(Pubkey::new_from_array(bytes))
    }

    /// Read 32 bytes as a `Pubkey` and return its base58 string.
    pub fn read_pubkey_as_string(&mut self) -> Result<String> {
        Ok(self.read_pubkey()?.to_string())
    }

    /// Read a borsh-style length-prefixed UTF-8 string (u32 length prefix).
    pub fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).context("invalid utf8 string")
    }

    /// Read a borsh-style 1-byte option tag (0=None, 1=Some).
    pub fn read_option_tag(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => bail!("invalid option discriminant {v}"),
        }
    }

    /// Read an optional pubkey (1-byte tag + 32-byte pubkey if present).
    pub fn read_option_pubkey(&mut self) -> Result<Option<Pubkey>> {
        if !self.read_option_tag()? {
            return Ok(None);
        }
        Ok(Some(self.read_pubkey()?))
    }
}

/// Bridges `BinaryReader` errors to `Ok(None)` for instruction parsing.
///
/// On `BinaryReader` error returns `Ok(None)`.
/// On success returns `Ok(Some(result))`.
#[allow(dead_code)]
pub(crate) fn try_parse<F>(data: &[u8], f: F) -> Result<Option<ParsedInstruction>>
where
    F: FnOnce(&mut BinaryReader) -> Result<ParsedInstruction>,
{
    let mut reader = BinaryReader::new(data);
    match f(&mut reader) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_u8_returns_correct_value() {
        let data = [0x42];
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_u8().unwrap(), 0x42);
    }

    #[test]
    fn read_u16_little_endian() {
        let data = 0x1234u16.to_le_bytes();
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_u16().unwrap(), 0x1234);
    }

    #[test]
    fn read_u32_little_endian() {
        let data = 0xDEAD_BEEFu32.to_le_bytes();
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_u32().unwrap(), 0xDEAD_BEEF);
    }

    #[test]
    fn read_u64_little_endian() {
        let data = 0x0102_0304_0506_0708u64.to_le_bytes();
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_u64().unwrap(), 0x0102_0304_0506_0708);
    }

    #[test]
    fn read_pubkey_returns_correct_pubkey() {
        let expected = Pubkey::new_unique();
        let data: Vec<u8> = expected.to_bytes().to_vec();
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_pubkey().unwrap(), expected);
    }

    #[test]
    fn read_pubkey_as_string_returns_base58() {
        let expected = Pubkey::new_unique();
        let data: Vec<u8> = expected.to_bytes().to_vec();
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_pubkey_as_string().unwrap(), expected.to_string());
    }

    #[test]
    fn read_string_length_prefixed_utf8() {
        let s = "hello";
        let mut data = Vec::new();
        data.extend_from_slice(&(s.len() as u32).to_le_bytes());
        data.extend_from_slice(s.as_bytes());
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_string().unwrap(), "hello");
    }

    #[test]
    fn read_bool_false_and_true() {
        let data = [0u8, 1u8];
        let mut reader = BinaryReader::new(&data);
        assert!(!reader.read_bool().unwrap());
        assert!(reader.read_bool().unwrap());
    }

    #[test]
    fn read_option_pubkey_some() {
        let pk = Pubkey::new_unique();
        let mut data = vec![1u8]; // tag = Some
        data.extend_from_slice(pk.as_ref());
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_option_pubkey().unwrap(), Some(pk));
    }

    #[test]
    fn read_option_pubkey_none() {
        let data = [0u8]; // tag = None
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.read_option_pubkey().unwrap(), None);
    }

    #[test]
    fn read_exact_insufficient_bytes_errors() {
        let data = [0u8; 2];
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_exact(5).is_err());
    }

    #[test]
    fn read_bool_invalid_value_errors() {
        let data = [2u8];
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_bool().is_err());
    }

    #[test]
    fn read_string_invalid_utf8_errors() {
        let mut data = Vec::new();
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&[0xFF, 0xFE, 0xFD]); // invalid utf8
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_string().is_err());
    }

    #[test]
    fn read_option_pubkey_invalid_tag_errors() {
        let data = [2u8]; // invalid tag
        let mut reader = BinaryReader::new(&data);
        assert!(reader.read_option_pubkey().is_err());
    }

    #[test]
    fn has_remaining_returns_false_at_end() {
        let data = [1u8];
        let mut reader = BinaryReader::new(&data);
        assert!(reader.has_remaining());
        reader.read_u8().unwrap();
        assert!(!reader.has_remaining());
    }

    #[test]
    fn remaining_and_offset_track_position() {
        let data = [0u8; 10];
        let mut reader = BinaryReader::new(&data);
        assert_eq!(reader.offset(), 0);
        assert_eq!(reader.remaining(), 10);
        reader.read_exact(4).unwrap();
        assert_eq!(reader.offset(), 4);
        assert_eq!(reader.remaining(), 6);
    }

    #[test]
    fn try_parse_returns_some_on_success() {
        let data = [0u8; 4];
        let result = try_parse(&data, |reader| {
            let _ = reader.read_u32()?;
            Ok(ParsedInstruction {
                name: "Test".into(),
                fields: vec![],
                account_names: vec![],
            })
        })
        .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "Test");
    }

    #[test]
    fn try_parse_returns_none_on_failure() {
        let data = [0u8; 1]; // too short for u32
        let result = try_parse(&data, |reader| {
            let _ = reader.read_u32()?;
            Ok(ParsedInstruction {
                name: "Test".into(),
                fields: vec![],
                account_names: vec![],
            })
        })
        .unwrap();
        assert!(result.is_none());
    }
}
