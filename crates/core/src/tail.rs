//! Incremental reads of append-only JSONL transcripts.
//!
//! The tailer remembers how far it has read each file and only ever returns
//! whole lines, so a line the harness is still writing is picked up on the
//! next read. Offsets are persisted by the caller (see `Store::save_offset`)
//! so a restart does not replay old lines.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Upper bound for one read, so a huge backlog is processed in steps.
pub const MAX_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct Tailer {
    offsets: HashMap<PathBuf, u64>,
}

/// New complete lines of a file.
#[derive(Debug, PartialEq, Eq)]
pub struct Chunk {
    /// Byte offset of `bytes` within the file.
    pub offset: u64,
    pub bytes: Vec<u8>,
    /// Whether more complete lines may follow right away.
    pub more: bool,
}

impl Tailer {
    pub fn new(offsets: HashMap<PathBuf, u64>) -> Self {
        Self { offsets }
    }

    pub fn offset(&self, path: &Path) -> u64 {
        self.offsets.get(path).copied().unwrap_or(0)
    }

    pub fn set_offset(&mut self, path: &Path, offset: u64) {
        self.offsets.insert(path.to_owned(), offset);
    }

    pub fn forget(&mut self, path: &Path) {
        self.offsets.remove(path);
    }

    /// Reads complete lines added since the last call. Returns `None` when
    /// nothing new is complete yet. A file that shrank is read from the start.
    pub fn read(&mut self, path: &Path) -> io::Result<Option<Chunk>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut offset = self.offset(path);
        if len < offset {
            offset = 0;
        }
        if len == offset {
            return Ok(None);
        }
        let want = (len - offset).min(MAX_CHUNK_BYTES);
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::with_capacity(want as usize);
        file.take(want).read_to_end(&mut bytes)?;

        let complete = match bytes.iter().rposition(|&b| b == b'\n') {
            Some(last) => last + 1,
            // A single line longer than the chunk limit can never complete
            // within it: skip past it instead of stalling forever.
            None if want == MAX_CHUNK_BYTES => {
                self.offsets.insert(path.to_owned(), offset + want);
                return Ok(None);
            }
            None => return Ok(None),
        };
        bytes.truncate(complete);
        let next = offset + complete as u64;
        self.offsets.insert(path.to_owned(), next);
        Ok(Some(Chunk {
            offset,
            bytes,
            more: next < len,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn returns_only_complete_new_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut file = File::create(&path).unwrap();
        let mut tailer = Tailer::default();

        assert_eq!(tailer.read(&path).unwrap(), None);
        file.write_all(b"{\"a\":1}\n{\"b\"").unwrap();
        let chunk = tailer.read(&path).unwrap().unwrap();
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.bytes, b"{\"a\":1}\n");
        assert!(chunk.more);
        assert_eq!(tailer.read(&path).unwrap(), None, "partial line waits");

        file.write_all(b":2}\n").unwrap();
        let chunk = tailer.read(&path).unwrap().unwrap();
        assert_eq!(chunk.offset, 8);
        assert_eq!(chunk.bytes, b"{\"b\":2}\n");
        assert!(!chunk.more);
    }

    #[test]
    fn restarts_truncated_files_and_resumes_from_saved_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        std::fs::write(&path, b"one\ntwo\n").unwrap();

        let mut tailer = Tailer::new(HashMap::from([(path.clone(), 4)]));
        assert_eq!(tailer.read(&path).unwrap().unwrap().bytes, b"two\n");

        std::fs::write(&path, b"x\n").unwrap();
        let chunk = tailer.read(&path).unwrap().unwrap();
        assert_eq!((chunk.offset, chunk.bytes.as_slice()), (0, &b"x\n"[..]));
    }
}
