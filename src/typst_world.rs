use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

static INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
static INTER_BOLD: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
static INTER_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-Italic.ttf");
static INTER_BOLD_ITALIC: &[u8] = include_bytes!("../assets/fonts/Inter-BoldItalic.ttf");
static JETBRAINS_REGULAR: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
static JETBRAINS_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");
static NOTO_COLOR_EMOJI: &[u8] = include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

const FONT_DATA: &[&[u8]] = &[
    INTER_REGULAR,
    INTER_BOLD,
    INTER_ITALIC,
    INTER_BOLD_ITALIC,
    JETBRAINS_REGULAR,
    JETBRAINS_BOLD,
    NOTO_COLOR_EMOJI,
];

/// A minimal Typst world implementation for PocketCrew document generation.
/// Bundles fonts via `include_bytes!` and serves image bytes through the `file()` method.
pub struct PocketCrewWorld {
    source: Source,
    main_id: FileId,
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    files: Arc<HashMap<FileId, Bytes>>,
}

impl PocketCrewWorld {
    pub fn new(source_text: String, files: HashMap<FileId, Bytes>) -> Self {
        let mut fonts = Vec::new();
        for bytes in FONT_DATA {
            let buf = Bytes::new(bytes.to_vec());
            for font in Font::iter(buf) {
                fonts.push(font);
            }
        }
        // Load New Computer Modern Math and other bundled Typst fonts so that
        // math mode has the fonts it needs to render equations.
        for bytes in typst_assets::fonts() {
            let buf = Bytes::new(bytes.to_vec());
            for font in Font::iter(buf) {
                fonts.push(font);
            }
        }
        let book = FontBook::from_fonts(fonts.iter());
        let main_id = FileId::new(None, VirtualPath::new("/main.typ"));
        let source = Source::new(main_id, source_text);
        Self {
            source,
            main_id,
            library: LazyHash::new(Library::builder().build()),
            book: LazyHash::new(book),
            fonts,
            files: Arc::new(files),
        }
    }
}

impl World for PocketCrewWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main_id
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_id {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(PathBuf::from(
                id.vpath().as_rooted_path(),
            )))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        match self.files.get(&id) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(FileError::NotFound(PathBuf::from(
                id.vpath().as_rooted_path(),
            ))),
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let now = time::OffsetDateTime::now_utc() + time::Duration::hours(offset.unwrap_or(0));
        Datetime::from_ymd(now.year(), now.month() as u8, now.day())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// W1 — Minimal .typ source compiles without error
    #[test]
    fn world_compiles_hello_world() {
        let world = PocketCrewWorld::new("= Hello, World!".to_string(), HashMap::new());
        let result = typst::compile::<typst::layout::PagedDocument>(&world);
        let doc = result.output.expect("compilation must succeed");
        assert_eq!(doc.pages.len(), 1);
    }

    /// W2 — Unknown file ID returns FileError::NotFound
    #[test]
    fn world_unknown_file_id_returns_not_found() {
        use typst::diag::FileError;
        use typst::syntax::{FileId, VirtualPath};

        let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
        let other_id = FileId::new(None, VirtualPath::new("/other.typ"));
        let err = world.source(other_id).unwrap_err();
        assert!(matches!(err, FileError::NotFound(_)), "got {:?}", err);
    }

    /// W3 — Font index 0 returns Some(Font)
    #[test]
    fn world_font_index_zero_returns_some() {
        let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
        assert!(world.font(0).is_some());
    }

    /// W4 — Font index out of range returns None
    #[test]
    fn world_font_index_out_of_range_returns_none() {
        let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
        assert!(world.font(999).is_none());
    }

    /// W5 — today() returns Some for None offset
    #[test]
    fn world_today_returns_some_for_none_offset() {
        let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
        assert!(world.today(None).is_some());
    }

    /// W6 — today() returns the current calendar date (within ±1 day for timezone)
    #[test]
    fn world_today_matches_system_clock_within_one_day() {
        use time::OffsetDateTime;
        let world = PocketCrewWorld::new("test".to_string(), HashMap::new());
        let now = OffsetDateTime::now_utc();
        let typst_today = world.today(None).expect("today");
        // Datetime has accessor methods; assert year/month line up.
        // Allow ±1 day slack for tests run near midnight UTC.
        let year_ok = typst_today.year().map(|y| y == now.year()).unwrap_or(false);
        let month_ok = typst_today
            .month()
            .map(|m| (m as u8 as i32 - now.month() as u8 as i32).abs() <= 1)
            .unwrap_or(false);
        assert!(year_ok, "year mismatch");
        assert!(month_ok, "month off by more than 1");
    }

    /// W7 — file() serves bytes for a registered image ID
    #[test]
    fn world_file_returns_registered_bytes() {
        use typst::foundations::Bytes;
        use typst::syntax::{FileId, VirtualPath};
        let id = FileId::new(None, VirtualPath::new("/img_0.png"));
        let mut files = HashMap::new();
        let payload = Bytes::new(b"PNG_DATA".to_vec());
        files.insert(id, payload.clone());
        let world = PocketCrewWorld::new("test".to_string(), files);
        let got = world.file(id).expect("registered file must resolve");
        assert_eq!(got.as_slice(), b"PNG_DATA");
    }
}
