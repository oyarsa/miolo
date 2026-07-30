//! Writing a table back to the file it came from.
//!
//! Encoding is a pure function from a [`Table`] to bytes; only [`save`] touches
//! the filesystem, and it is deliberately cautious about doing so — an editor
//! that loses the file it was editing is worse than one that cannot save.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

use super::{Origin, Table, modified_time};
use crate::source::{Compression, Format};

/// Suffix of the temporary file a save is staged through.
const TEMP_SUFFIX: &str = ".miolo-tmp";

/// What a successful write leaves behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub path: PathBuf,
    /// The file's modification time afterwards. This becomes the baseline the
    /// next write checks against — without carrying it back, miolo's own save
    /// would look like someone else's and every write after the first would be
    /// refused.
    pub modified: Option<SystemTime>,
}

/// Why this table cannot be written back, in a form fit for the status bar.
///
/// Reported when the editor opens rather than when the user tries to save, so
/// nobody types forty lines into something that was never going to persist.
pub fn blocker(origin: &Origin) -> Option<&'static str> {
    if origin.path.is_none() {
        return Some("input came from standard input");
    }
    if origin.compression != Compression::None {
        return Some("the input is compressed");
    }
    match origin.format {
        // Loading flattens every JSON value to its display text, and fills in
        // keys a record never had. Writing that back would turn numbers,
        // nulls and nested objects into strings — a silent corruption of the
        // file, which is worse than refusing.
        Format::Json | Format::Jsonl => Some("JSON values are flattened to text when loaded"),
        Format::Delimited(_) => None,
    }
}

/// Encode a table in its own format.
pub fn encode(table: &Table) -> Result<Vec<u8>> {
    match table.origin.format {
        Format::Delimited(delimiter) => encode_delimited(table, delimiter),
        Format::Json | Format::Jsonl => {
            bail!("cannot write JSON back: JSON values are flattened to text when loaded")
        }
    }
}

/// Encode a table as separated values.
///
/// The output is a faithful representation of the table in memory, which is
/// not always a faithful representation of the file that was read: short rows
/// were padded at load and surplus columns were given `+1` names, so a ragged
/// input is written back square.
fn encode_delimited(table: &Table, delimiter: u8) -> Result<Vec<u8>> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_writer(Vec::new());

    writer.write_record(&table.headers)?;
    for row in &table.rows {
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(writer.into_inner()?)
}

/// Write the table back to its file.
pub fn save(table: &Table) -> Result<Saved> {
    if let Some(reason) = blocker(&table.origin) {
        bail!("cannot write this input back: {reason}");
    }
    let Some(path) = table.origin.path.as_deref() else {
        bail!("no file to write to");
    };

    // Someone else may have written the file while it was open. Overwriting
    // would discard their work silently, so refuse and let the user decide.
    if modified_time(path) != table.origin.modified {
        bail!(
            "{} changed on disk since it was loaded",
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
        );
    }

    let bytes = encode(table)?;
    write_atomically(path, &bytes)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(Saved {
        path: path.to_path_buf(),
        modified: modified_time(path),
    })
}

/// Write through a sibling temporary file and rename over the target.
///
/// A rename within a directory is atomic, so an interrupted save leaves either
/// the old file or the new one — never a half-written mixture of the two.
///
/// Every way of failing takes the staging file with it, including a write that
/// ran out of disk part-way. What no process can tidy up after is being killed
/// outright; a staging file left by that is inert, and the next save overwrites
/// it, so it is not worth deleting files at startup to chase.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = temp_path(path);
    let staged = stage(&temp, path, bytes);
    if staged.is_err() {
        let _ = fs::remove_file(&temp);
    }
    staged
}

/// Write the staging file, give it the target's permissions, and move it over.
fn stage(temp: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(temp, bytes)?;
    // Match the original's permissions; a fresh temp file gets the umask's.
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(temp, meta.permissions());
    }
    fs::rename(temp, path)?;
    Ok(())
}

/// A hidden sibling of the target, so the rename stays within one filesystem.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "miolo".to_owned(), |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}{TEMP_SUFFIX}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_csv;

    fn delimited(csv: &str) -> Table {
        let mut table = parse_csv(csv.as_bytes(), "test").expect("parse failed");
        table.origin.path = Some(PathBuf::from("/tmp/test.csv"));
        table
    }

    fn encoded(table: &Table) -> String {
        String::from_utf8(encode(table).expect("encode failed")).expect("not UTF-8")
    }

    #[test]
    fn round_trips_a_simple_table() {
        assert_eq!(encoded(&delimited("a,b\n1,2\n")), "a,b\n1,2\n");
    }

    #[test]
    fn quotes_only_what_needs_it() {
        let table = delimited("a,b\n\"one,two\",\"line\nbreak\"\n");
        assert_eq!(encoded(&table), "a,b\n\"one,two\",\"line\nbreak\"\n");
    }

    #[test]
    fn keeps_the_delimiter_it_was_read_with() {
        let mut table = delimited("a,b\n1,2\n");
        table.origin.format = Format::Delimited(b'\t');
        assert_eq!(encoded(&table), "a\tb\n1\t2\n");
    }

    #[test]
    fn writes_edits_back() {
        let mut table = delimited("a,b\n1,2\n");
        table.set_field(0, 1, "edited".to_owned());
        assert_eq!(encoded(&table), "a,b\n1,edited\n");
    }

    #[test]
    fn ragged_input_is_written_back_square() {
        // Row one was padded and the surplus column named at load; the write
        // reflects the table, not the file.
        let table = delimited("a,b\n1\n2,3,4\n");
        assert_eq!(encoded(&table), "a,b,+1\n1,,\n2,3,4\n");
    }

    #[test]
    fn stdin_cannot_be_written_back() {
        let origin = Origin::default();
        assert_eq!(blocker(&origin), Some("input came from standard input"));
    }

    #[test]
    fn compressed_input_cannot_be_written_back() {
        let origin = Origin {
            path: Some(PathBuf::from("a.csv.gz")),
            compression: Compression::Gzip,
            ..Origin::default()
        };
        assert_eq!(blocker(&origin), Some("the input is compressed"));
    }

    #[test]
    fn json_cannot_be_written_back() {
        for format in [Format::Json, Format::Jsonl] {
            let origin = Origin {
                path: Some(PathBuf::from("a.json")),
                format,
                ..Origin::default()
            };
            assert!(blocker(&origin).is_some(), "{format:?} must be refused");
        }
    }

    #[test]
    fn a_plain_file_is_writable() {
        let origin = Origin {
            path: Some(PathBuf::from("a.csv")),
            ..Origin::default()
        };
        assert_eq!(blocker(&origin), None);
    }

    #[test]
    fn the_temporary_file_is_a_hidden_sibling() {
        let temp = temp_path(Path::new("/data/orders.csv"));
        assert_eq!(temp.parent(), Path::new("/data/orders.csv").parent());
        assert_eq!(
            temp.file_name().and_then(|n| n.to_str()),
            Some(".orders.csv.miolo-tmp")
        );
    }

    /// A scratch file that removes itself, so the round-trip tests below can
    /// use a real filesystem without a dependency or a leftover.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str, contents: &str) -> Self {
            let path = std::env::temp_dir().join(format!("miolo-{}-{tag}.csv", std::process::id()));
            fs::write(&path, contents).expect("could not write the scratch file");
            Self(path)
        }

        /// The table as `load` would have set it up, pointing at this file.
        fn table(&self) -> Table {
            let contents = fs::read(&self.0).expect("could not read the scratch file");
            let mut table = parse_csv(&contents, "scratch").expect("parse failed");
            table.origin = Origin {
                path: Some(self.0.clone()),
                modified: modified_time(&self.0),
                ..Origin::default()
            };
            table
        }

        fn contents(&self) -> String {
            fs::read_to_string(&self.0).expect("could not read the scratch file")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn saving_writes_the_edit_to_the_file() {
        let scratch = Scratch::new("round-trip", "a,b\n1,2\n");
        let mut table = scratch.table();
        table.set_field(0, 1, "edited".to_owned());

        save(&table).expect("save failed");
        assert_eq!(scratch.contents(), "a,b\n1,edited\n");
    }

    #[test]
    fn a_second_save_is_not_mistaken_for_someone_elses_change() {
        // The first write changes the file's modification time. Unless that
        // new time is carried back as the baseline, the staleness check sees
        // miolo's own edit as an outside one and refuses every write after
        // the first.
        let scratch = Scratch::new("resave", "a,b\n1,2\n");
        let mut table = scratch.table();

        table.set_field(0, 1, "first".to_owned());
        let saved = save(&table).expect("first save failed");
        table.origin.modified = saved.modified;

        table.set_field(0, 1, "second".to_owned());
        save(&table).expect("second save failed");
        assert_eq!(scratch.contents(), "a,b\n1,second\n");
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let scratch = Scratch::new("no-temp", "a,b\n1,2\n");
        save(&scratch.table()).expect("save failed");
        assert!(
            !temp_path(&scratch.0).exists(),
            "the staging file must be renamed away"
        );
    }

    #[test]
    fn a_failed_write_takes_its_staging_file_with_it() {
        // A directory in place of the target: the rename cannot succeed, which
        // stands in for any other way the move can fail.
        let dir = std::env::temp_dir().join(format!("miolo-{}-blocked.csv", std::process::id()));
        fs::create_dir_all(&dir).expect("could not create the blocking directory");

        let error = write_atomically(&dir, b"a,b\n").expect_err("should not overwrite a directory");
        assert!(
            !temp_path(&dir).exists(),
            "a failed save must not leave its staging file: {error}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_refuses_when_the_file_changed_underneath() {
        // A path that does not exist reads back no modification time, which
        // will not match the one recorded at load.
        let mut table = delimited("a,b\n1,2\n");
        table.origin.path = Some(PathBuf::from("/nonexistent/orders.csv"));
        table.origin.modified = Some(std::time::UNIX_EPOCH);
        let error = save(&table).expect_err("should refuse");
        assert!(error.to_string().contains("changed on disk"), "{error}");
    }
}
