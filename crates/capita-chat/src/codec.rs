// SPDX-License-Identifier: AGPL-3.0-only
//! A small reader and writer for the byte formats of this crate. All
//! integers little-endian; lengths as `u16` or `u32` before the bytes.

use crate::ChatError;

/// Reads a byte string from the front, never past its end.
pub(crate) struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    pub(crate) fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], ChatError> {
        let (head, tail) = self
            .0
            .split_at_checked(n)
            .ok_or(ChatError::Malformed(what))?;
        self.0 = tail;
        Ok(head)
    }

    pub(crate) fn array<const N: usize>(
        &mut self,
        what: &'static str,
    ) -> Result<[u8; N], ChatError> {
        let bytes = self.take(N, what)?;
        <[u8; N]>::try_from(bytes).map_err(|_| ChatError::Malformed(what))
    }

    pub(crate) fn u8(&mut self, what: &'static str) -> Result<u8, ChatError> {
        Ok(u8::from_le_bytes(self.array::<1>(what)?))
    }

    pub(crate) fn u16(&mut self, what: &'static str) -> Result<u16, ChatError> {
        Ok(u16::from_le_bytes(self.array::<2>(what)?))
    }

    pub(crate) fn u32(&mut self, what: &'static str) -> Result<u32, ChatError> {
        Ok(u32::from_le_bytes(self.array::<4>(what)?))
    }

    /// Bytes with a `u16` length in front.
    pub(crate) fn short(&mut self, what: &'static str) -> Result<&'a [u8], ChatError> {
        let n = self.u16(what)?;
        self.take(usize::from(n), what)
    }

    /// Bytes with a `u32` length in front.
    pub(crate) fn long(&mut self, what: &'static str) -> Result<&'a [u8], ChatError> {
        let n = usize::try_from(self.u32(what)?).map_err(|_| ChatError::Malformed(what))?;
        self.take(n, what)
    }

    pub(crate) fn end(&self, what: &'static str) -> Result<(), ChatError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(ChatError::Malformed(what))
        }
    }
}

/// Appends bytes with a `u16` length in front.
pub(crate) fn put_short(
    out: &mut Vec<u8>,
    bytes: &[u8],
    what: &'static str,
) -> Result<(), ChatError> {
    let n = u16::try_from(bytes.len()).map_err(|_| ChatError::Malformed(what))?;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// Appends bytes with a `u32` length in front.
pub(crate) fn put_long(
    out: &mut Vec<u8>,
    bytes: &[u8],
    what: &'static str,
) -> Result<(), ChatError> {
    let n = u32::try_from(bytes.len()).map_err(|_| ChatError::Malformed(what))?;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

/// Appends an optional 32-byte id: `0`, or `1 ‖ id`.
pub(crate) fn put_option(out: &mut Vec<u8>, id: Option<&[u8; 32]>) {
    match id {
        None => out.push(0),
        Some(id) => {
            out.push(1);
            out.extend_from_slice(id);
        }
    }
}

/// Reads what [`put_option`] wrote.
pub(crate) fn option(
    r: &mut Reader<'_>,
    what: &'static str,
) -> Result<Option<[u8; 32]>, ChatError> {
    match r.u8(what)? {
        0 => Ok(None),
        1 => Ok(Some(r.array(what)?)),
        _ => Err(ChatError::Malformed(what)),
    }
}
