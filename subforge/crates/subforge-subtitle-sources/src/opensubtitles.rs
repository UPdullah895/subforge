use crate::SourceError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const CHUNK_SIZE: u64 = 65536; // 64 KiB

/// Computes OpenSubtitles' "moviehash"/OSHash: file size plus a little-endian
/// u64 checksum (wrapping add) of the first and last 64KiB of the file. Files
/// between 64KiB and 128KiB have the two windows overlap — that's expected
/// and still produces a valid hash, per the algorithm's spec.
pub fn compute_hash(path: &Path) -> Result<(String, u64), SourceError> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();
    if file_size < CHUNK_SIZE {
        return Err(SourceError::FileTooSmall);
    }

    let mut hash = file_size;
    hash = hash.wrapping_add(sum_chunk(&mut file, 0)?);
    hash = hash.wrapping_add(sum_chunk(&mut file, file_size - CHUNK_SIZE)?);

    Ok((format!("{hash:016x}"), file_size))
}

fn sum_chunk(file: &mut File, offset: u64) -> Result<u64, SourceError> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = [0u8; CHUNK_SIZE as usize];
    file.read_exact(&mut buf)?;
    let mut sum: u64 = 0;
    for eight_bytes in buf.chunks_exact(8) {
        sum = sum.wrapping_add(u64::from_le_bytes(eight_bytes.try_into().unwrap()));
    }
    Ok(sum)
}

#[cfg(test)]
mod hash_tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f
    }

    #[test]
    fn hashes_a_file_of_exactly_two_chunks() {
        // 128 KiB of zero bytes: head chunk sum = 0, tail chunk sum = 0,
        // so hash == file size, formatted as 16-digit zero-padded hex.
        let bytes = vec![0u8; 131072];
        let f = write_temp_file(&bytes);
        let (hash, size) = compute_hash(f.path()).unwrap();
        assert_eq!(size, 131072);
        assert_eq!(hash, format!("{:016x}", 131072u64));
    }

    #[test]
    fn hashes_a_file_smaller_than_128kib_with_overlapping_regions() {
        // 100 KiB: head and tail 64KiB windows overlap, but the hash must
        // still be well-defined (both windows are valid byte ranges).
        let bytes = vec![1u8; 100 * 1024];
        let f = write_temp_file(&bytes);
        let result = compute_hash(f.path());
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_files_smaller_than_64kib() {
        let bytes = vec![0u8; 1000];
        let f = write_temp_file(&bytes);
        let result = compute_hash(f.path());
        assert!(matches!(result, Err(crate::SourceError::FileTooSmall)));
    }
}
