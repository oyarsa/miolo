//! Decompression, as a step in front of parsing.
//!
//! Bytes in, bytes out — so the whole load pipeline stays testable without
//! touching the filesystem. Both decoders are pure Rust, which keeps
//! cross-compiling the macOS release target free of a C toolchain.

use std::io::{self, Read};

use crate::source::Compression;

/// Uncompress `data` according to `compression`.
///
/// Uncompressed input is returned untouched rather than copied.
pub fn decompress(data: Vec<u8>, compression: Compression) -> io::Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data),
        Compression::Gzip => read_all(flate2::read::GzDecoder::new(&data[..])),
        Compression::Zstd => {
            let decoder = ruzstd::decoding::StreamingDecoder::new(&data[..])
                .map_err(|error| io::Error::other(format!("not valid zstd: {error}")))?;
            read_all(decoder)
        }
    }
}

fn read_all(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SAMPLE: &[u8] = b"id,notes\n1,\"one\ntwo\"\n2,three\n";

    fn gzipped(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("gzip write failed");
        encoder.finish().expect("gzip finish failed")
    }

    #[test]
    fn uncompressed_data_passes_through() {
        let out = decompress(SAMPLE.to_vec(), Compression::None).expect("decompress failed");
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn gzip_round_trips() {
        let compressed = gzipped(SAMPLE);
        assert_ne!(compressed, SAMPLE, "fixture really is compressed");
        let out = decompress(compressed, Compression::Gzip).expect("decompress failed");
        assert_eq!(out, SAMPLE);
    }

    #[test]
    fn gzip_survives_a_payload_larger_than_one_buffer() {
        let big: Vec<u8> = SAMPLE.iter().copied().cycle().take(200_000).collect();
        let out = decompress(gzipped(&big), Compression::Gzip).expect("decompress failed");
        assert_eq!(out, big);
    }

    #[test]
    fn corrupt_gzip_is_an_error_not_a_panic() {
        let mut corrupt = gzipped(SAMPLE);
        let tail = corrupt.len() - 4;
        corrupt.truncate(tail);
        assert!(decompress(corrupt, Compression::Gzip).is_err());
    }

    #[test]
    fn data_that_is_not_zstd_is_an_error() {
        let error =
            decompress(SAMPLE.to_vec(), Compression::Zstd).expect_err("plain text is not zstd");
        assert!(error.to_string().contains("zstd"), "error names the format");
    }
}
