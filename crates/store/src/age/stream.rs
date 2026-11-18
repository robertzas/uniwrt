//! age's STREAM payload: 64 KiB chunks, each sealed with a counter nonce, the last one flagged.

use std::io::{self, BufRead, BufReader, Read, Write};

use ring::aead::{self, Aad, LessSafeKey};

use super::{
    FileKey, NONCE, TAG, chacha, derive, io_error, malformed, open_header, random, seal_header,
};
use crate::Result;

/// Plaintext per chunk.
pub(super) const CHUNK: usize = 64 * 1024;

fn payload_key(file_key: &FileKey, nonce: &[u8; NONCE]) -> LessSafeKey {
    chacha(&derive(file_key, nonce, b"payload"))
}

fn chunk_nonce(counter: u64, last: bool) -> aead::Nonce {
    let mut nonce = [0; 12];
    nonce[3..11].copy_from_slice(&counter.to_be_bytes());
    nonce[11] = u8::from(last);
    aead::Nonce::assume_unique_for_key(nonce)
}

/// Encrypts what is written to it into `out`; [`Encryptor::finish`] seals the last chunk.
pub struct Encryptor<W: Write> {
    out: W,
    key: LessSafeKey,
    counter: u64,
    /// The chunk being filled, with room for its tag.
    chunk: Vec<u8>,
}

impl<W: Write> Encryptor<W> {
    /// Writes the header for `passphrase` with work factor `log_n` (normally [`LOG_N`](super::LOG_N)).
    pub fn new(mut out: W, passphrase: &[u8], log_n: u8) -> Result<Self> {
        let (mut header, file_key) = seal_header(passphrase, log_n)?;
        let mut nonce = [0; NONCE];
        random(&mut nonce)?;
        header.extend_from_slice(&nonce);
        out.write_all(&header).map_err(|source| crate::Error::Io {
            what: "the encrypted file".to_owned(),
            source,
        })?;
        Ok(Self {
            out,
            key: payload_key(&file_key, &nonce),
            counter: 0,
            chunk: Vec::with_capacity(CHUNK + TAG),
        })
    }

    fn seal(&mut self, last: bool) -> io::Result<()> {
        self.key
            .seal_in_place_append_tag(
                chunk_nonce(self.counter, last),
                Aad::empty(),
                &mut self.chunk,
            )
            .map_err(|_| io_error(malformed("cannot seal a chunk")))?;
        self.out.write_all(&self.chunk)?;
        self.chunk.clear();
        self.counter += 1;
        Ok(())
    }

    /// Seals the last chunk (empty only if nothing was written) and returns the output.
    pub fn finish(mut self) -> io::Result<W> {
        self.seal(true)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

impl<W: Write> Write for Encryptor<W> {
    fn write(&mut self, mut data: &[u8]) -> io::Result<usize> {
        let len = data.len();
        while !data.is_empty() {
            // A full chunk is sealed only once more follows: the last one must be flagged.
            if self.chunk.len() == CHUNK {
                self.seal(false)?;
            }
            let take = (CHUNK - self.chunk.len()).min(data.len());
            self.chunk.extend_from_slice(&data[..take]);
            data = &data[take..];
        }
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Decrypts an age file read from `input`; reading fails on a truncated or altered payload, so
/// what was read is trusted only once a read returns 0.
pub struct Decryptor<R: Read> {
    input: BufReader<R>,
    key: LessSafeKey,
    counter: u64,
    /// The sealed chunk read (and the first byte of the next, which tells it isn't the last).
    sealed: Vec<u8>,
    /// The opened chunk and how much of it was read.
    plain: Vec<u8>,
    at: usize,
    done: bool,
}

impl<R: Read> Decryptor<R> {
    /// Reads the header and opens it with `passphrase`: [`Error::Passphrase`](crate::Error::Passphrase)
    /// when it doesn't, [`Error::NotAge`](crate::Error::NotAge) when the file isn't one this reads.
    pub fn new(input: R, passphrase: &[u8]) -> Result<Self> {
        let mut input = BufReader::with_capacity(CHUNK + TAG + 1, input);
        let file_key = open_header(&mut input, passphrase)?;
        let mut nonce = [0; NONCE];
        input
            .read_exact(&mut nonce)
            .map_err(|_| malformed("no payload"))?;
        Ok(Self {
            input,
            key: payload_key(&file_key, &nonce),
            counter: 0,
            sealed: Vec::with_capacity(CHUNK + TAG + 1),
            plain: Vec::with_capacity(CHUNK + TAG),
            at: 0,
            done: false,
        })
    }

    /// Reads and opens the next chunk into `plain`.
    fn next_chunk(&mut self) -> io::Result<()> {
        // Up to a full sealed chunk and one byte more: that byte means another chunk follows.
        while self.sealed.len() <= CHUNK + TAG {
            let buf = self.input.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            let take = buf.len().min(CHUNK + TAG + 1 - self.sealed.len());
            self.sealed.extend_from_slice(&buf[..take]);
            self.input.consume(take);
        }
        let last = self.sealed.len() <= CHUNK + TAG;
        let len = self.sealed.len().min(CHUNK + TAG);
        // Only an empty file's one chunk may be empty; a truncated file ends here too.
        if len < TAG || (last && len == TAG && self.counter > 0) {
            return Err(io_error(malformed("truncated payload")));
        }
        self.plain.clear();
        self.plain.extend_from_slice(&self.sealed[..len]);
        self.sealed.drain(..len);
        let opened = self
            .key
            .open_in_place(
                chunk_nonce(self.counter, last),
                Aad::empty(),
                &mut self.plain,
            )
            .map_err(|_| io_error(malformed("the payload was truncated or altered")))?
            .len();
        self.plain.truncate(opened);
        self.at = 0;
        self.counter += 1;
        self.done = last;
        Ok(())
    }
}

impl<R: Read> Read for Decryptor<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.at == self.plain.len() {
            if self.done {
                return Ok(0);
            }
            self.next_chunk()?;
        }
        let n = buf.len().min(self.plain.len() - self.at);
        buf[..n].copy_from_slice(&self.plain[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}
