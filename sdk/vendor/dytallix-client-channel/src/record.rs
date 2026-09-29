//! Records: AES-256-GCM with one key per direction and a counter nonce.
//!
//! A record is a 16-byte header (`DYCR`, version, flags, eight-byte
//! big-endian sequence, two-byte big-endian plaintext length) and the
//! ciphertext with its tag. The whole header is the associated data, so the
//! end-of-message flag is authenticated: a message cut short never completes.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::Error;

pub const RECORD_HEADER_LEN: usize = 16;
pub const MAX_RECORD_PLAINTEXT: usize = 16384;
pub const TAG_LEN: usize = 16;
/// Records per direction. A connection never wraps or reuses a nonce.
pub const MAX_RECORDS: u64 = 1 << 20;

const MAGIC: &[u8; 4] = b"DYCR";
const VERSION: u8 = 1;
const END: u8 = 0x01;

fn nonce(sequence: u64) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[4..].copy_from_slice(&sequence.to_be_bytes());
    out
}

fn cipher(key: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new(&(*key).into())
}

/// Seals what one side sends.
pub struct Sealer {
    cipher: Aes256Gcm,
    sequence: u64,
}

impl Sealer {
    pub(crate) fn new(key: &[u8; 32]) -> Self {
        Sealer {
            cipher: cipher(key),
            sequence: 0,
        }
    }

    /// Seals one whole, non-empty message as records of at most 16 KiB; the
    /// last is marked as the end of the message.
    pub fn seal_message(&mut self, message: &[u8]) -> Result<Vec<u8>, Error> {
        if message.is_empty() {
            return Err(Error::Malformed);
        }
        let count = message.len().div_ceil(MAX_RECORD_PLAINTEXT) as u64;
        if self.sequence + count > MAX_RECORDS {
            return Err(Error::Limit);
        }
        let mut out =
            Vec::with_capacity(message.len() + count as usize * (RECORD_HEADER_LEN + TAG_LEN));
        for (index, chunk) in message.chunks(MAX_RECORD_PLAINTEXT).enumerate() {
            let mut header = [0u8; RECORD_HEADER_LEN];
            header[..4].copy_from_slice(MAGIC);
            header[4] = VERSION;
            header[5] = if index as u64 + 1 == count { END } else { 0 };
            header[6..14].copy_from_slice(&self.sequence.to_be_bytes());
            header[14..].copy_from_slice(&(chunk.len() as u16).to_be_bytes());
            let sealed = self
                .cipher
                .encrypt(
                    &Nonce::from(nonce(self.sequence)),
                    Payload {
                        msg: chunk,
                        aad: &header,
                    },
                )
                .map_err(|_| Error::Limit)?;
            out.extend_from_slice(&header);
            out.extend_from_slice(&sealed);
            self.sequence += 1;
        }
        Ok(out)
    }
}

/// Opens what the other side sends. Use it through a [`MessageReader`].
pub struct Opener {
    cipher: Aes256Gcm,
    sequence: u64,
}

impl Opener {
    pub(crate) fn new(key: &[u8; 32]) -> Self {
        Opener {
            cipher: cipher(key),
            sequence: 0,
        }
    }

    /// Checks a header before its ciphertext is read: the form, the next
    /// exact sequence and the length. Returns the end flag and the length.
    fn check(&self, header: &[u8; RECORD_HEADER_LEN]) -> Result<(bool, usize), Error> {
        if &header[..4] != MAGIC || header[4] != VERSION || header[5] & !END != 0 {
            return Err(Error::Malformed);
        }
        if self.sequence >= MAX_RECORDS {
            return Err(Error::Limit);
        }
        let sequence = u64::from_be_bytes(header[6..14].try_into().expect("eight bytes"));
        if sequence != self.sequence {
            return Err(Error::Rejected);
        }
        let len = u16::from_be_bytes([header[14], header[15]]) as usize;
        if len == 0 || len > MAX_RECORD_PLAINTEXT {
            return Err(Error::Malformed);
        }
        Ok((header[5] & END != 0, len))
    }

    fn open(&mut self, header: &[u8; RECORD_HEADER_LEN], sealed: &[u8]) -> Result<Vec<u8>, Error> {
        let plain = self
            .cipher
            .decrypt(
                &Nonce::from(nonce(self.sequence)),
                Payload {
                    msg: sealed,
                    aad: header,
                },
            )
            .map_err(|_| Error::Rejected)?;
        self.sequence += 1;
        Ok(plain)
    }
}

/// Reassembles one message from records. Read [`wanted`](Self::wanted)
/// bytes and [`feed`](Self::feed) them until the message is returned. Any
/// error is final: close the connection.
pub struct MessageReader {
    opener: Opener,
    limit: usize,
    buffer: Vec<u8>,
    pending: Option<([u8; RECORD_HEADER_LEN], bool, usize)>,
    finished: bool,
}

impl MessageReader {
    /// `limit` bounds the whole message, checked from each header before
    /// its ciphertext is read.
    pub fn new(opener: Opener, limit: usize) -> Self {
        MessageReader {
            opener,
            limit,
            buffer: Vec::new(),
            pending: None,
            finished: false,
        }
    }

    /// The exact byte count to read next: a header, or the current record's
    /// ciphertext and tag.
    pub fn wanted(&self) -> usize {
        match self.pending {
            None => RECORD_HEADER_LEN,
            Some((_, _, len)) => len + TAG_LEN,
        }
    }

    /// Feeds exactly [`wanted`](Self::wanted) bytes. Returns the message
    /// once its last record is authenticated.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let result = self.step(bytes);
        if result.is_err() {
            self.finished = true;
            self.buffer.clear();
        }
        result
    }

    fn step(&mut self, bytes: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        if self.finished || bytes.len() != self.wanted() {
            return Err(Error::Malformed);
        }
        match self.pending.take() {
            None => {
                let header: [u8; RECORD_HEADER_LEN] = bytes.try_into().expect("checked length");
                let (end, len) = self.opener.check(&header)?;
                if self.buffer.len() + len > self.limit {
                    return Err(Error::Limit);
                }
                self.pending = Some((header, end, len));
                Ok(None)
            }
            Some((header, end, _)) => {
                let plain = self.opener.open(&header, bytes)?;
                self.buffer.extend_from_slice(&plain);
                if end {
                    self.finished = true;
                    Ok(Some(std::mem::take(&mut self.buffer)))
                } else {
                    Ok(None)
                }
            }
        }
    }
}
