//! 单源、磁盘与多文件内存覆盖共用的编译入口。
use crate::{analysis, lexer, parser, CompileResult, Diagnostic, Span};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

pub fn source_path(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let canonical = absolute.canonicalize().unwrap_or_else(|_| {
        match (absolute.parent(), absolute.file_name()) {
            (Some(parent), Some(name)) if parent != absolute => source_path(parent).join(name),
            _ => absolute,
        }
    });
    let mut normal = PathBuf::new();
    // Windows canonicalize 的设备路径前缀不应使新建文件与已保存文件身份不同。
    let display = canonical.to_string_lossy();
    let canonical = Path::new(display.strip_prefix(r"\\?\").unwrap_or(&display));
    for part in canonical.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            _ => normal.push(part.as_os_str()),
        }
    }
    normal
}

pub fn entry_path(path: &Path) -> PathBuf {
    source_path(&if path.is_dir() {
        path.join("world.wl")
    } else {
        path.to_path_buf()
    })
}

/// 单源文本不加载 include;多文件编辑请使用 compile_sources。
///
/// ```
/// let result = worldline_core::compile_source("hello.wl", "event hello\n  你好,世界。\n  -> END\n");
/// assert!(!result.has_errors());
/// assert_eq!(result.program.entry, "hello");
/// ```
pub fn compile_source(file: &str, src: &str) -> CompileResult {
    let mut diags = Vec::new();
    let lines = lexer::lex_source(file, src, &mut diags);
    finish(
        file,
        lines,
        vec![file.into()],
        BTreeMap::from([(PathBuf::from(file), src.into())]),
        diags,
    )
}

pub fn compile_path(path: &Path) -> std::io::Result<CompileResult> {
    if path.is_dir() {
        let root = source_path(path);
        let mut sources = BTreeMap::new();
        for file in crate::file_access::workspace_files(&root)? {
            if file.extension().is_some_and(|e| e == "wl") {
                sources.insert(file.clone(), crate::file_access::read_to_string(file)?);
            }
        }
        crate::file_access::read_to_string(root.join("world.wl"))?;
        return Ok(compile_sources(&root.join("world.wl"), &sources));
    }
    let path = entry_path(path);
    let text = crate::file_access::read_to_string(&path)?;
    Ok(compile_text_with_disk_includes(&path, &text))
}

pub fn compile_text_with_disk_includes(path: &Path, text: &str) -> CompileResult {
    compile_sources(path, &BTreeMap::from([(source_path(path), text.into())]))
}

/// 所有内存文件优先于磁盘,包括尚未保存的新文件。
pub fn compile_sources(entry: &Path, sources: &BTreeMap<PathBuf, String>) -> CompileResult {
    let entry = entry_path(entry);
    let overrides = sources
        .iter()
        .map(|(p, text)| (source_path(p), text.clone()))
        .collect();
    let mut compiler = Compiler {
        root: entry.parent().unwrap_or(Path::new(".")).to_path_buf(),
        overrides,
        sources: BTreeMap::new(),
        files: Vec::new(),
        loaded: HashSet::new(),
        active: Vec::new(),
        lines: Vec::new(),
        diags: Vec::new(),
    };
    compiler.load(&entry, &entry.to_string_lossy(), Span::new(1, 1, 1));
    for path in compiler.overrides.keys().cloned().collect::<Vec<_>>() {
        compiler.load(&path, &entry.to_string_lossy(), Span::new(1, 1, 1));
    }
    finish(
        &entry.to_string_lossy(),
        compiler.lines,
        compiler.files,
        compiler.sources,
        compiler.diags,
    )
}

fn finish(
    main: &str,
    lines: Vec<lexer::Line>,
    files: Vec<String>,
    sources: BTreeMap<PathBuf, String>,
    mut diags: Vec<Diagnostic>,
) -> CompileResult {
    let mut program = parser::Parser::new(&lines, &mut diags).parse_program();
    program.files = files;
    let entry = program
        .event_files
        .iter()
        .position(|f| f == main)
        .or_else(|| {
            program
                .files
                .iter()
                .find_map(|f| program.event_files.iter().position(|ef| ef == f))
        })
        .unwrap_or(0);
    program.entry = program
        .events
        .get(entry)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    crate::migration::normalize(&mut program, &sources);
    let (analysis, diagnostics) = analysis::analyze(&program, diags);
    CompileResult {
        program,
        analysis,
        diagnostics,
        sources,
    }
}

struct Compiler {
    root: PathBuf,
    overrides: BTreeMap<PathBuf, String>,
    sources: BTreeMap<PathBuf, String>,
    files: Vec<String>,
    loaded: HashSet<PathBuf>,
    active: Vec<PathBuf>,
    lines: Vec<lexer::Line>,
    diags: Vec<Diagnostic>,
}

impl Compiler {
    fn load(&mut self, path: &Path, from: &str, span: Span) {
        let path = source_path(path);
        if !path.starts_with(&self.root) {
            self.diags.push(Diagnostic::error(
                "A109",
                from,
                span,
                "引用文件必须位于工作区目录内",
            ));
            return;
        }
        if self.active.contains(&path) || self.active.len() >= 128 {
            self.diags.push(Diagnostic::error(
                "A105",
                from,
                span,
                format!("include 构成环路或嵌套过深:{}", path.display()),
            ));
            return;
        }
        if self.loaded.contains(&path) {
            return;
        }
        let text = match self
            .overrides
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| crate::file_access::read_to_string(&path))
        {
            Ok(text) => text,
            Err(e) => {
                self.diags.push(Diagnostic::error(
                    "A105",
                    from,
                    span,
                    format!("文件无法读取:{} ({e})", path.display()),
                ));
                return;
            }
        };
        self.loaded.insert(path.clone());
        self.active.push(path.clone());
        self.sources.insert(path.clone(), text.clone());
        let display = path.to_string_lossy().into_owned();
        self.files.push(display.clone());
        for line in lexer::lex_source(&display, &text, &mut self.diags) {
            if let lexer::LineKind::Include {
                path: include,
                span,
            } = &line.kind
            {
                if line.indent != 0 {
                    self.diags.push(Diagnostic::error(
                        "P002",
                        &display,
                        *span,
                        "include 只能出现在文件顶层",
                    ));
                } else if Path::new(include).is_absolute() {
                    self.diags.push(Diagnostic::error(
                        "A109",
                        &display,
                        *span,
                        "include 必须使用工作区内相对路径",
                    ));
                } else {
                    self.load(
                        &path.parent().unwrap_or(Path::new(".")).join(include),
                        &display,
                        *span,
                    );
                }
            } else {
                self.lines.push(line);
            }
        }
        self.active.pop();
    }
}
