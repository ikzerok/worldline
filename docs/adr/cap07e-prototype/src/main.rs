//! Disposable format experiment. Never loads a worldline Project.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

struct Meter;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
}
#[global_allocator]
static ALLOC: Meter = Meter;

fn preflight(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    if bytes.len() > 1_048_576 {
        return Err("package_limit".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() > 64 {
        return Err("entry_count_limit".into());
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_owned();
        if name.starts_with('/')
            || name.contains('\\')
            || name.contains(':')
            || name.split('/').any(|p| p == "..")
            || !seen.insert(name.clone())
        {
            return Err("unsafe_or_duplicate_path".into());
        }
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("symlink".into());
        }
        if name.ends_with(".bin") {
            return Err("binary_part_rejected".into());
        }
        if entry.size() > 4_194_304 {
            return Err("entry_limit".into());
        }
        if entry.size() > entry.compressed_size().max(1).saturating_mul(100) {
            return Err("ratio_limit".into());
        }
        let mut part = Vec::new();
        (&mut entry).take(4_194_305).read_to_end(&mut part)?;
        if part.len() > 4_194_304 {
            return Err("actual_entry_limit".into());
        }
        total += part.len() as u64;
        if total > 8_388_608 {
            return Err("total_limit".into());
        }
        if name.ends_with(".xml") || name.ends_with(".rels") || name.ends_with(".xhtml") {
            let xml = std::str::from_utf8(&part)?;
            if xml.contains("<!DOCTYPE") || xml.contains("<!ENTITY") {
                return Err("dtd_rejected".into());
            }
        }
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("docx") => {
            let mut bytes = Vec::new();
            File::open(&args[2])?
                .take(1_048_577)
                .read_to_end(&mut bytes)?;
            preflight(&bytes)?;
            let doc = docx_rs::read_docx(&bytes)?;
            doc.build().pack(File::create(&args[3])?)?;
        }
        Some("epub") => {
            use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ZipLibrary};
            let xhtml = std::fs::read(&args[2])?;
            let mut book = EpubBuilder::new(ZipLibrary::new()?)?;
            book.epub_version(EpubVersion::V30)
                .metadata("title", "白塔与星海 🌌")?
                .metadata("author", "固定样例")?
                .metadata("lang", "zh-CN")?
                .stylesheet(&b"h1 { color: navy; }"[..])?
                .add_resource(
                    "images/pixel.png",
                    &std::fs::read(&args[3])?[..],
                    "image/png",
                )?
                .add_content(EpubContent::new("chapter.xhtml", &xhtml[..]).title("第一章"))?;
            book.generate(File::create(&args[4])?)?;
        }
        _ => return Err("usage: docx input output | epub xhtml png output".into()),
    }
    Ok(())
}

fn main() {
    let start = Instant::now();
    let result = run();
    eprintln!(
        "elapsed_us={} peak_rust_heap_bytes={}",
        start.elapsed().as_micros(),
        PEAK.load(Ordering::Relaxed)
    );
    if let Err(error) = result {
        eprintln!("rejected={error}");
        std::process::exit(1);
    }
}
