//! Resolving an input to a compression container and a data format.
//!
//! The two are resolved by different means on purpose. Compression is
//! *detected* from magic bytes, which are an unambiguous identification. The
//! data format is *declared* — by the file extension or `--format` — and is
//! never guessed from content, because a wrong guess reports a parse failure
//! against the wrong format, which is confusing exactly when the user has
//! already made a mistake.

use std::path::Path;

use clap::ValueEnum;

/// Container wrapping the data, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zstd,
}

/// How the bytes are structured, once uncompressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Separated values, carrying the separator byte.
    Delimited(u8),
    /// A single array of objects.
    Json,
    /// One object per line.
    Jsonl,
}

/// The `--format` values a user can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    Csv,
    Tsv,
    Psv,
    Json,
    Jsonl,
}

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Extensions stripped before the format extension is read, so that
/// `orders.json.gz` resolves to gzip plus JSON.
const COMPRESSION_EXTENSIONS: &[&str] = &["gz", "zst", "zstd"];

/// Identify the compression container from the leading bytes.
///
/// These signatures cannot begin a text file, so this is identification rather
/// than inference. Detecting it also means a compressed file with a misleading
/// name still opens, and that piped input needs no flag.
pub fn detect_compression(data: &[u8]) -> Compression {
    if data.starts_with(&GZIP_MAGIC) {
        Compression::Gzip
    } else if data.starts_with(&ZSTD_MAGIC) {
        Compression::Zstd
    } else {
        Compression::None
    }
}

/// Drop a trailing compression extension, if there is one.
fn strip_compression_extension(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for suffix in COMPRESSION_EXTENSIONS {
        let dotted = format!(".{suffix}");
        if lower.ends_with(&dotted) {
            return &name[..name.len() - dotted.len()];
        }
    }
    name
}

/// The format a path's extension implies, if it is one we recognise.
pub fn format_from_path(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_str()?;
    let stem = strip_compression_extension(name);
    let extension = Path::new(stem).extension()?.to_str()?.to_ascii_lowercase();

    match extension.as_str() {
        "csv" => Some(Format::Delimited(b',')),
        "tsv" | "tab" => Some(Format::Delimited(b'\t')),
        "psv" => Some(Format::Delimited(b'|')),
        "json" => Some(Format::Json),
        "jsonl" | "ndjson" => Some(Format::Jsonl),
        _ => None,
    }
}

/// The separator a declared delimited format defaults to.
fn default_delimiter(arg: FormatArg) -> u8 {
    match arg {
        FormatArg::Tsv => b'\t',
        FormatArg::Psv => b'|',
        _ => b',',
    }
}

/// Settle on a format from the flags and the filename.
///
/// `--format` wins, then an explicit `--delimiter`, then the extension, and
/// failing all of those, comma-separated — which keeps piped input behaving as
/// it always has.
pub fn resolve_format(
    path: Option<&Path>,
    requested: Option<FormatArg>,
    delimiter: Option<u8>,
) -> Format {
    if let Some(arg) = requested {
        return match arg {
            FormatArg::Json => Format::Json,
            FormatArg::Jsonl => Format::Jsonl,
            separated => {
                Format::Delimited(delimiter.unwrap_or_else(|| default_delimiter(separated)))
            }
        };
    }
    if let Some(byte) = delimiter {
        return Format::Delimited(byte);
    }
    path.and_then(format_from_path)
        .unwrap_or(Format::Delimited(b','))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(name: &str) -> Format {
        resolve_format(Some(Path::new(name)), None, None)
    }

    #[test]
    fn detects_compression_from_magic_bytes() {
        assert_eq!(
            detect_compression(&[0x1f, 0x8b, 0x08, 0x00]),
            Compression::Gzip
        );
        assert_eq!(
            detect_compression(&[0x28, 0xb5, 0x2f, 0xfd, 0x00]),
            Compression::Zstd
        );
        assert_eq!(detect_compression(b"id,name\n1,a\n"), Compression::None);
    }

    #[test]
    fn short_input_is_not_mistaken_for_a_container() {
        assert_eq!(detect_compression(b""), Compression::None);
        assert_eq!(detect_compression(&[0x1f]), Compression::None);
        assert_eq!(detect_compression(&[0x28, 0xb5]), Compression::None);
    }

    #[test]
    fn extensions_map_to_formats() {
        assert_eq!(resolved("a.csv"), Format::Delimited(b','));
        assert_eq!(resolved("a.tsv"), Format::Delimited(b'\t'));
        assert_eq!(resolved("a.tab"), Format::Delimited(b'\t'));
        assert_eq!(resolved("a.psv"), Format::Delimited(b'|'));
        assert_eq!(resolved("a.json"), Format::Json);
        assert_eq!(resolved("a.jsonl"), Format::Jsonl);
        assert_eq!(resolved("a.ndjson"), Format::Jsonl);
    }

    #[test]
    fn extensions_are_case_insensitive() {
        assert_eq!(resolved("A.CSV"), Format::Delimited(b','));
        assert_eq!(resolved("A.JSONL"), Format::Jsonl);
    }

    #[test]
    fn compression_extensions_are_stripped_first() {
        assert_eq!(resolved("a.json.gz"), Format::Json);
        assert_eq!(resolved("a.jsonl.zst"), Format::Jsonl);
        assert_eq!(resolved("a.tsv.zstd"), Format::Delimited(b'\t'));
        assert_eq!(resolved("a.csv.GZ"), Format::Delimited(b','));
    }

    #[test]
    fn unknown_and_absent_extensions_fall_back_to_csv() {
        assert_eq!(resolved("a.txt"), Format::Delimited(b','));
        assert_eq!(resolved("data"), Format::Delimited(b','));
        assert_eq!(resolved("a.gz"), Format::Delimited(b','));
    }

    #[test]
    fn stdin_without_flags_is_csv() {
        assert_eq!(resolve_format(None, None, None), Format::Delimited(b','));
    }

    #[test]
    fn the_format_flag_beats_the_extension() {
        let path = Some(Path::new("a.csv"));
        assert_eq!(
            resolve_format(path, Some(FormatArg::Json), None),
            Format::Json
        );
        assert_eq!(
            resolve_format(path, Some(FormatArg::Tsv), None),
            Format::Delimited(b'\t')
        );
    }

    #[test]
    fn an_explicit_delimiter_implies_a_delimited_format() {
        assert_eq!(
            resolve_format(Some(Path::new("a.txt")), None, Some(b';')),
            Format::Delimited(b';')
        );
    }

    #[test]
    fn the_delimiter_refines_a_declared_delimited_format() {
        assert_eq!(
            resolve_format(None, Some(FormatArg::Csv), Some(b';')),
            Format::Delimited(b';')
        );
    }

    #[test]
    fn a_delimiter_cannot_turn_json_into_a_table() {
        assert_eq!(
            resolve_format(None, Some(FormatArg::Json), Some(b';')),
            Format::Json,
            "the declared format wins outright"
        );
    }

    #[test]
    fn an_extension_does_not_override_an_explicit_delimiter() {
        assert_eq!(
            resolve_format(Some(Path::new("a.tsv")), None, Some(b';')),
            Format::Delimited(b';')
        );
    }
}
