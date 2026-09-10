//! 从已有资料生成关键词和源码出现位置；不改写正文或建立语义引用。
use crate::catalog::TargetRef;
use crate::navigation::RenderedLink;
use crate::project::{Project, SearchHit};
use crate::CompileResult;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordMatch {
    /// 原文字节范围，可直接切片；出现位置的列号另按 Unicode 字符计数。
    pub start: usize,
    pub end: usize,
    pub targets: Vec<TargetRef>,
}

#[derive(Default)]
pub struct KeywordIndex {
    keywords: BTreeMap<char, Vec<(String, Vec<TargetRef>)>>,
    occurrences: BTreeMap<TargetRef, Vec<SearchHit>>,
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl KeywordIndex {
    pub fn new(result: &CompileResult) -> Self {
        let catalog = &result.analysis.catalog;
        let mut names: BTreeMap<String, BTreeSet<TargetRef>> = BTreeMap::new();
        for object in &catalog.objects {
            if matches!(object.target.kind.as_str(), "file" | "variable")
                || (object.target.kind == "tag"
                    && !catalog
                        .tags
                        .get(&object.target.id)
                        .is_some_and(|t| t.declared))
            {
                continue;
            }
            for name in
                std::iter::once(object.display.clone()).chain(catalog.aliases_for(&object.target))
            {
                let name = name.trim();
                if !name.is_empty() && !name.contains(['\n', '\r']) {
                    names
                        .entry(name.to_ascii_lowercase())
                        .or_default()
                        .insert(object.target.clone());
                }
            }
        }
        let mut index = Self::default();
        for (name, targets) in names {
            index
                .keywords
                .entry(name.chars().next().unwrap())
                .or_default()
                .push((name, targets.into_iter().collect()));
        }
        for bucket in index.keywords.values_mut() {
            bucket.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        }
        index.index_sources(result);
        index
    }

    /// 左侧优先、同一位置最长优先；重复出现和同名候选全部保留。
    pub fn find(&self, text: &str) -> Vec<KeywordMatch> {
        self.find_with_links(text, &[])
    }

    /// 显式链接优先，自动匹配不能跨越或覆盖它的显示文字。
    pub fn find_with_links(&self, text: &str, explicit: &[RenderedLink]) -> Vec<KeywordMatch> {
        let mut links: Vec<_> = explicit
            .iter()
            .filter(|link| {
                link.start < link.end
                    && text.is_char_boundary(link.start)
                    && text.is_char_boundary(link.end)
            })
            .collect();
        links.sort_by_key(|link| link.start);
        let mut links = links.into_iter().peekable();
        let folded = text.to_ascii_lowercase();
        let mut matches = Vec::new();
        let mut end = 0;
        for (start, first) in folded.char_indices() {
            if start < end {
                continue;
            }
            while links.peek().is_some_and(|link| link.start < start) {
                links.next();
            }
            if links.peek().is_some_and(|link| link.start == start) {
                let link = links.next().unwrap();
                matches.push(KeywordMatch {
                    start,
                    end: link.end,
                    targets: vec![link.target.clone()],
                });
                end = link.end;
                continue;
            }
            let Some(bucket) = self.keywords.get(&first) else {
                continue;
            };
            for (name, targets) in bucket {
                let next = start + name.len();
                if !folded[start..].starts_with(name)
                    || links.peek().is_some_and(|link| next > link.start)
                    || (word(first) && folded[..start].chars().next_back().is_some_and(word))
                    || (name.chars().next_back().is_some_and(word)
                        && folded[next..].chars().next().is_some_and(word))
                {
                    continue;
                }
                matches.push(KeywordMatch {
                    start,
                    end: next,
                    targets: targets.clone(),
                });
                end = next;
                break;
            }
        }
        matches
    }

    pub fn occurrences(&self, target: &TargetRef) -> &[SearchHit] {
        self.occurrences
            .get(target)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn index_sources(&mut self, result: &CompileResult) {
        let mut links = BTreeMap::new();
        for link in &result.analysis.catalog.text_links {
            links
                .entry((link.file.as_str(), link.line))
                .or_insert_with(Vec::new)
                .push(link);
        }
        for (file, source) in &result.sources {
            let cleaned = crate::lexer::strip_comments(source);
            let choice_columns: BTreeMap<_, _> =
                crate::lexer::lex_source(&file.to_string_lossy(), source, &mut Vec::new())
                    .into_iter()
                    .filter_map(|line| match line.kind {
                        crate::lexer::LineKind::Choice { label_span, .. } => {
                            Some((line.no, line.indent + label_span.column))
                        }
                        _ => None,
                    })
                    .collect();
            for (number, (raw, line)) in source.lines().zip(cleaned.lines()).enumerate() {
                let line_number = number as u32 + 1;
                let mut explicit = Vec::new();
                if let Some(line_links) = links.get(&(file.to_string_lossy().as_ref(), line_number))
                {
                    for link in line_links {
                        let column = if let Some(&base) = choice_columns.get(&line_number) {
                            // 选择文案先解码外层字符串，需把位置映回带转义的原始源码。
                            let mut chars = line.chars().skip(base.saturating_sub(1) as usize);
                            let mut column = base;
                            for _ in base..link.column {
                                let escaped = chars.next() == Some('\\');
                                column += 1;
                                if escaped && chars.next().is_some() {
                                    column += 1;
                                }
                            }
                            column
                        } else {
                            link.column
                        };
                        let start = line
                            .char_indices()
                            .nth(column.saturating_sub(1) as usize)
                            .map(|(offset, _)| offset)
                            .unwrap_or(line.len());
                        let end = line[start..]
                            .find("]]")
                            .map(|offset| start + offset + 2)
                            .unwrap_or(line.len());
                        explicit.push(RenderedLink {
                            start,
                            end,
                            target: link.target.clone(),
                        });
                    }
                }
                for found in self.find_with_links(line, &explicit) {
                    let column = line[..found.start].chars().count() as u32 + 1;
                    for target in found.targets {
                        self.occurrences.entry(target).or_default().push(SearchHit {
                            file: file.clone(),
                            line: line_number,
                            column,
                            preview: raw.into(),
                        });
                    }
                }
            }
        }
        for hits in self.occurrences.values_mut() {
            hits.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
            hits.dedup_by(|a, b| a.file == b.file && a.line == b.line && a.column == b.column);
        }
    }
}

impl Project {
    /// 使用 Project::edit 提交词条及别名，失败时一起回滚。
    pub fn write_wiki_entry(
        &mut self,
        original: Option<&str>,
        draft: &crate::authoring::WorldDraft,
        aliases: &[String],
    ) -> Result<(), String> {
        if draft.display.trim().is_empty() || draft.display.contains(['\n', '\r']) {
            return Err("关键词不能为空白或包含换行".into());
        }
        self.write_tag(original, draft)?;
        self.set_aliases(&TargetRef::new("tag", &draft.id), aliases)
    }
}
