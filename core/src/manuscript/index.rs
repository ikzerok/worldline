use super::source::{resolve_perspective, resolve_source, SourceLookup};
use super::*;
use crate::workspace_documents::parse_unique_json;
use crate::{CompileResult, Span};
use std::collections::HashMap;

/// 从已注册文档的原始字节建立只读书稿索引。
///
/// `registered_read_only` 由清单能力协商和 Project 文档保护状态提供；
/// `registered_id` 是清单中的键。该函数不访问磁盘，也不修改 Project。
pub fn build_manuscript_index(
    bytes: &[u8],
    file: &str,
    registered_id: &str,
    manifest_required_features: &[String],
    registered_read_only: bool,
    content: &CompileResult,
) -> ManuscriptIndex {
    let mut index = ManuscriptIndex {
        id: None,
        title: None,
        entries: Vec::new(),
        diagnostics: Vec::new(),
        read_only: registered_read_only,
        file: file.to_string(),
        source_bytes: bytes.to_vec(),
        source_document: None,
        chapter_order: Vec::new(),
        original_parents: Vec::new(),
        original_parent_valid: Vec::new(),
        original_id_counts: std::collections::BTreeMap::new(),
    };
    let document = match parse_unique_json(bytes) {
        Ok(document) => document,
        Err(error) => {
            index.error("MAN001", format!("书稿 JSON 无法解析：{error}"));
            return index;
        }
    };
    index.source_document = Some(document.clone());
    let Some(object) = document.as_object() else {
        index.error("MAN001", "书稿顶层必须是 JSON 对象");
        return index;
    };

    let Some(schema_version) = object.get("schema_version").and_then(Value::as_u64) else {
        index.error("MAN001", "书稿 schema_version 必须是整数");
        return index;
    };
    if schema_version != MANUSCRIPT_SCHEMA_VERSION {
        index.read_only = true;
        index.error(
            "MAN002",
            format!("书稿格式版本 {schema_version} 不受支持，按只读处理"),
        );
        return index;
    }
    if !manifest_required_features
        .iter()
        .any(|feature| feature == MANUSCRIPT_REQUIRED_FEATURE)
    {
        index.read_only = true;
        index.error(
            "MAN002",
            format!("清单未声明必需能力 `{MANUSCRIPT_REQUIRED_FEATURE}`，书稿按只读处理"),
        );
        return index;
    }
    if registered_read_only {
        index.error("MAN002", "书稿注册能力不受支持，按只读处理");
        return index;
    }

    let Some(id) = object.get("id").and_then(Value::as_str) else {
        index.error("MAN001", "书稿 id 必须是字符串");
        return index;
    };
    if !valid_id(id) {
        index.error("MAN001", "书稿 id 格式无效");
        return index;
    }
    index.id = Some(id.to_string());
    if id != registered_id {
        index.error(
            "MAN001",
            format!("书稿 id `{id}` 与清单注册 ID `{registered_id}` 不一致"),
        );
        return index;
    }
    let title = match object.get("title").and_then(Value::as_str) {
        Some(title) if !title.trim().is_empty() => title,
        _ => {
            index.error("MAN001", "书稿 title 必须是非空字符串");
            id
        }
    };
    index.title = Some(title.to_string());
    let Some(entries) = object.get("entries").and_then(Value::as_array) else {
        index.error("MAN001", "书稿 entries 必须是数组");
        return index;
    };

    // 身份先于kind识别：无法投影的坏项仍可能与合法项争用同一ID。
    for value in entries {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            *index.original_id_counts.entry(id.to_owned()).or_default() += 1;
        }
    }
    let duplicates: Vec<_> = index
        .original_id_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, count)| (id.clone(), *count))
        .collect();
    for (id, count) in duplicates {
        for _ in 1..count {
            index.error("MAN005", format!("书稿节点 ID `{id}` 重复"));
        }
    }
    for value in entries {
        let Some(node) = value.as_object() else {
            index.error("MAN001", "书稿 entries 中的每项必须是对象");
            continue;
        };
        let Some(entry_id) = node.get("id").and_then(Value::as_str) else {
            index.error("MAN001", "书稿节点 id 必须是字符串");
            continue;
        };
        if !valid_id(entry_id) {
            index.error("MAN001", format!("书稿节点 ID `{entry_id}` 格式无效"));
            continue;
        }
        let kind = match node.get("kind").and_then(Value::as_str) {
            Some("section") => ManuscriptEntryKind::Section,
            Some("chapter") => ManuscriptEntryKind::Chapter,
            _ => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 kind 只允许 section 或 chapter"),
                );
                continue;
            }
        };
        let node_title = match node.get("title").and_then(Value::as_str) {
            Some(title) if !title.trim().is_empty() => title,
            _ => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 title 必须是非空字符串"),
                );
                entry_id
            }
        };

        let parent_id = parse_optional_string(node, "parent_id", entry_id, &mut index);
        let summary = parse_optional_string(node, "summary", entry_id, &mut index);
        let status = parse_optional_string(node, "status", entry_id, &mut index);
        let goal = parse_optional_string(node, "goal", entry_id, &mut index);
        let perspective = parse_optional_target(node, "pov", entry_id, &mut index);
        let target_ref = parse_optional_target(node, "target_ref", entry_id, &mut index);

        match kind {
            ManuscriptEntryKind::Section if target_ref.is_some() => {
                index.error("MAN008", format!("section `{entry_id}` 不能引用正文目标"));
            }
            ManuscriptEntryKind::Section if node.contains_key("pov") => {
                index.error("MAN008", format!("section `{entry_id}` 不能声明 POV"));
            }
            ManuscriptEntryKind::Chapter if target_ref.is_none() => {
                index.error(
                    "MAN001",
                    format!("chapter `{entry_id}` 必须声明 target_ref"),
                );
            }
            _ => {}
        }

        index.original_parent_valid.push(matches!(
            node.get("parent_id"),
            None | Some(Value::Null) | Some(Value::String(_))
        ));
        index.entries.push(ManuscriptEntry {
            id: entry_id.to_string(),
            kind,
            parent_id,
            title: node_title.to_string(),
            summary,
            perspective,
            perspective_status: None,
            status,
            goal,
            target_ref,
            source: None,
        });
    }

    index.original_parents = index
        .entries
        .iter()
        .map(|entry| entry.parent_id.clone())
        .collect();
    validate_hierarchy(&mut index);
    let lookup = SourceLookup::new(content);
    let mut sources = std::collections::BTreeMap::new();
    let mut perspectives = std::collections::BTreeMap::new();
    for entry_index in 0..index.entries.len() {
        if index.entries[entry_index].kind != ManuscriptEntryKind::Chapter {
            continue;
        }
        if let Some(target) = index.entries[entry_index].target_ref.clone() {
            let (source, diagnostics) = sources.entry(target.clone()).or_insert_with(|| {
                let start = index.diagnostics.len();
                let source = resolve_source(&target, content, &lookup, &mut index);
                let diagnostics = index.diagnostics.drain(start..).collect::<Vec<_>>();
                (source, diagnostics)
            });
            index.diagnostics.extend(diagnostics.iter().cloned());
            let source = source.clone();
            index.entries[entry_index].source = Some(source);
        }
        if let Some(perspective) = index.entries[entry_index].perspective.clone() {
            let (status, diagnostics) =
                perspectives.entry(perspective.clone()).or_insert_with(|| {
                    let start = index.diagnostics.len();
                    let status = resolve_perspective(&perspective, content, &lookup, &mut index);
                    let diagnostics = index.diagnostics.drain(start..).collect::<Vec<_>>();
                    (status, diagnostics)
                });
            index.diagnostics.extend(diagnostics.iter().cloned());
            let status = *status;
            index.entries[entry_index].perspective_status = Some(status);
        }
    }
    build_chapter_order(&mut index);
    crate::diagnostic::sort_diagnostics(&mut index.diagnostics);
    index
}
impl ManuscriptIndex {
    pub(super) fn error(&mut self, code: &'static str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            code,
            &self.file,
            Span::new(1, 1, 1),
            message,
        ));
    }
}

fn parse_optional_string(
    node: &serde_json::Map<String, Value>,
    field: &str,
    entry_id: &str,
    index: &mut ManuscriptIndex,
) -> Option<String> {
    match node.get(field) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            index.error(
                "MAN001",
                format!("书稿节点 `{entry_id}` 的 {field} 必须是字符串"),
            );
            None
        }
    }
}

fn parse_optional_target(
    node: &serde_json::Map<String, Value>,
    field: &str,
    entry_id: &str,
    index: &mut ManuscriptIndex,
) -> Option<TargetRef> {
    match node.get(field) {
        None | Some(Value::Null) => None,
        Some(value) => match parse_target(value) {
            Some(target) => Some(target),
            None => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 {field} 必须含非空 kind 与 id"),
                );
                None
            }
        },
    }
}

fn parse_target(value: &Value) -> Option<TargetRef> {
    let object = value.as_object()?;
    let kind = object.get("kind")?.as_str()?;
    let id = object.get("id")?.as_str()?;
    if kind.is_empty() || id.is_empty() {
        return None;
    }
    Some(TargetRef::new(kind, id))
}

pub(super) fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn validate_hierarchy(index: &mut ManuscriptIndex) {
    let mut first_by_id = HashMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        first_by_id.entry(entry.id.clone()).or_insert(position);
    }
    let mut parents = vec![None; index.entries.len()];
    for (position, parent_slot) in parents.iter_mut().enumerate() {
        let entry_id = index.entries[position].id.clone();
        let Some(parent_id) = index.entries[position].parent_id.clone() else {
            continue;
        };
        let Some(parent_index) = first_by_id.get(&parent_id).copied() else {
            index.error(
                "MAN006",
                format!("节点 `{entry_id}` 的父 section `{parent_id}` 不存在"),
            );
            continue;
        };
        if parent_index == position {
            index.error("MAN006", format!("节点 `{entry_id}` 不能成为自己的父项"));
            continue;
        }
        let parent_is_section = index.entries[parent_index].kind == ManuscriptEntryKind::Section;
        if !parent_is_section {
            index.error(
                "MAN006",
                format!("节点 `{entry_id}` 的父项 `{parent_id}` 必须是 section"),
            );
            continue;
        }
        *parent_slot = Some(parent_index);
    }

    let mut complete = vec![false; parents.len()];
    for start in 0..parents.len() {
        if complete[start] {
            continue;
        }
        let mut path: Vec<usize> = Vec::new();
        let mut positions: HashMap<usize, usize> = HashMap::new();
        let mut current: Option<usize> = Some(start);
        while let Some(node) = current {
            if complete[node] {
                break;
            }
            if positions.contains_key(&node) {
                if let Some(edge_source) = path.last().copied() {
                    parents[edge_source] = None;
                    index.error(
                        "MAN007",
                        format!(
                            "书稿层级在节点 `{}` 处形成循环",
                            index.entries[edge_source].id
                        ),
                    );
                }
                break;
            }
            positions.insert(node, path.len());
            path.push(node);
            current = parents[node];
        }
        for node in path {
            complete[node] = true;
        }
    }

    // 校验后的父索引由顺序投影使用；无效父项按根节点展示，避免隐藏章节。
    for (position, parent) in parents.into_iter().enumerate() {
        let parent_id = parent.map(|parent| index.entries[parent].id.clone());
        index.entries[position].parent_id = parent_id;
    }
}

fn build_chapter_order(index: &mut ManuscriptIndex) {
    let mut first_by_id = HashMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        first_by_id.entry(entry.id.clone()).or_insert(position);
    }
    let mut children = vec![Vec::new(); index.entries.len()];
    let mut roots = Vec::new();
    for (position, entry) in index.entries.iter().enumerate() {
        match entry
            .parent_id
            .as_ref()
            .and_then(|parent_id| first_by_id.get(parent_id).copied())
        {
            Some(parent) => children[parent].push(position),
            None => roots.push(position),
        }
    }

    fn walk(
        position: usize,
        entries: &[ManuscriptEntry],
        children: &[Vec<usize>],
        visited: &mut [bool],
        order: &mut Vec<(usize, Vec<String>)>,
    ) {
        let mut section_path = Vec::new();
        let mut stack = vec![(position, false)];
        while let Some((node, leaving)) = stack.pop() {
            if leaving {
                section_path.pop();
                continue;
            }
            if visited[node] {
                continue;
            }
            visited[node] = true;
            let entry = &entries[node];
            if entry.kind == ManuscriptEntryKind::Chapter {
                order.push((node, section_path.clone()));
            } else {
                section_path.push(entry.id.clone());
                stack.push((node, true));
            }
            for child in children[node].iter().rev() {
                stack.push((*child, false));
            }
        }
    }

    let mut visited = vec![false; index.entries.len()];
    for root in roots {
        walk(
            root,
            &index.entries,
            &children,
            &mut visited,
            &mut index.chapter_order,
        );
    }
    // 重复 ID 造成的歧义节点仍可见，按文档顺序追加；原始数组不被改写。
    for position in 0..index.entries.len() {
        if !visited[position] {
            walk(
                position,
                &index.entries,
                &children,
                &mut visited,
                &mut index.chapter_order,
            );
        }
    }
}
