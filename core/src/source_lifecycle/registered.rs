//! 仅迁移已登记、已通过正式读取器校验的源码路径；未知可选字段保持原字节。
use super::Failure;
use super::SourceLifecycleChange;
use crate::catalog::TargetRef;
use crate::collaboration::CommentIndex;
use crate::project::Project;
use crate::refactor::{documents, json_spans, preview};
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, Registry};
use crate::{Diagnostic, Severity};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathKind {
    Identity,
    Relative,
    PositionKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PathSlot {
    target: PathBuf,
    kind: PathKind,
}
type Inventory = BTreeMap<String, PathSlot>;

pub(super) fn rewrite(
    project: &Project,
    old: &Path,
    new: &Path,
    mut remaining_paths: usize,
) -> Result<Vec<SourceLifecycleChange>, String> {
    let registry = checked_registry(project)?;
    validate_documents(project, &registry)?;
    let target = TargetRef::new("file", old.to_str().ok_or("旧源码路径不是 UTF-8")?);
    let new_id = new.to_str().ok_or("新源码路径不是 UTF-8")?;
    let relative = new
        .strip_prefix(&project.root)
        .map_err(|_| "新源码路径越过工作区边界")?
        .to_str()
        .ok_or("新源码相对路径不是 UTF-8")?
        .replace('\\', "/");
    let mut changes = Vec::new();
    for path in registry.documents.keys() {
        let document = project.authoring_document(path)?;
        let source = std::str::from_utf8(document.bytes())
            .map_err(|_| format!("已登记文档不是 UTF-8，无法安全迁移：{}", path.display()))?;
        let before = parse_unique_json(document.bytes()).map_err(|error| error.to_string())?;
        let inventory = inventory(project, &registry, path, &before)?;
        if inventory.len() > remaining_paths {
            return Err("源码与已登记文档正式路径合计超过 16384 项预算，工程未修改".into());
        }
        remaining_paths -= inventory.len();
        let mut after = before.clone();
        // 实际书稿 target_ref 不支持 file，POV 的 wire key 为 pov 且只支持 character。
        // 不能误改第三方可选字段 perspective；模板同样没有正式 file 引用。
        if !registered(&registry.manuscripts, path) && !registered(&registry.templates, path) {
            documents::rewrite_registered(&mut after, &registry, path, &target, new_id);
        }
        for (pointer, slot) in &inventory {
            if slot.kind == PathKind::Relative && slot.target == old {
                *after.pointer_mut(pointer).ok_or("已登记路径字段消失")? =
                    Value::String(relative.clone());
            }
        }
        verify_inventory(project, &registry, path, &inventory, &after, old, new)?;
        let edits = json_spans::edits(source, &before, &after, &target, new_id)?;
        let expected = inventory.values().filter(|slot| slot.target == old).count();
        if edits.len() != expected
            || edits.iter().any(|edit| {
                !inventory
                    .get(&edit.field)
                    .is_some_and(|slot| slot.target == old)
            })
        {
            return Err(format!(
                "已登记源码路径改写与正式字段清单不一致：{}",
                path.display()
            ));
        }
        if edits.is_empty() {
            continue;
        }
        let (text, occurrences) = preview::apply(source, edits)?;
        if parse_unique_json(text.as_bytes()).map_err(|error| error.to_string())? != after {
            return Err("已登记文档原始 token 改写与语义候选不一致".into());
        }
        changes.push(SourceLifecycleChange {
            path: path.clone(),
            after_path: path.clone(),
            kind: "authoring".into(),
            occurrences,
            before: Some(document.bytes().to_vec()),
            after: Some(text.into_bytes()),
        });
    }
    Ok(changes)
}

/// 必须在完整源码与 JSON 候选装入后检查；不重写 quote/hash 来假造批注确认。
pub(super) fn validate_candidate(before: &Project, after: &Project) -> Result<(), Failure> {
    let old_registry = checked_registry(before)?;
    let new_registry = checked_registry(after)?;
    let old_comments = validate_documents(before, &old_registry)?;
    let new_comments = validate_documents(after, &new_registry)?;
    for (id, comment) in old_comments.comments {
        let current = new_comments
            .comments
            .get(&id)
            .ok_or("源码移动丢失了已登记批注")?;
        if comment.anchor_status != current.anchor_status {
            return Err(Failure::semantic(format!(
                "源码移动将改变批注 `{id}` 的附着状态；引用行正文发生改动时不能自动重新确认 quote/hash"
            )));
        }
    }
    Ok(())
}

fn checked_registry(project: &Project) -> Result<Registry, String> {
    project.ensure_workspace_writable()?;
    let manifest = manifest_path(&project.root);
    let Some(document) = project
        .authoring_documents
        .get(&manifest)
        .filter(|document| !document.is_deleted())
    else {
        return Ok(Registry::default());
    };
    let registry = parse_registry(&project.root, document.bytes());
    check_diagnostics(&registry.diagnostics)?;
    if !registry.proposals.is_empty() {
        return Err("已登记 proposals 捕获了源码路径、摘要与基线；本版无法证明提案迁移等价，拒绝整个源码移动".into());
    }
    for (path, inherited) in &registry.documents {
        super::safety::writable_path(path)?;
        let document = project.authoring_document(path)?;
        if document.is_deleted() || document.is_read_only() || *inherited {
            return Err(format!(
                "已登记文档缺失或只读，无法安全移动源码：{}",
                path.display()
            ));
        }
        let value = parse_unique_json(document.bytes())
            .map_err(|error| format!("已登记文档 JSON 无效：{}：{error}", path.display()))?;
        if value.get("schema_version").and_then(Value::as_u64) != Some(1)
            || crate::workspace_documents::document_read_only(document.bytes(), *inherited)
        {
            return Err(format!(
                "已登记文档 schema 或 required_features 不受支持：{}",
                path.display()
            ));
        }
    }
    Ok(registry)
}

fn validate_documents(project: &Project, registry: &Registry) -> Result<CommentIndex, String> {
    let content = project.compile_current();
    let maps = crate::presentation::build_map_index(project, &content);
    check_diagnostics(&maps.diagnostics)?;
    let views = crate::graph_views::build_graph_view_index(project, &content);
    check_diagnostics(&views.diagnostics)?;
    let presets = crate::presentation_presets::build_preset_index(project, &content, &maps, &views);
    check_diagnostics(&presets.diagnostics)?;
    let comments = crate::collaboration::build_comment_index(project, &content, &maps);
    check_diagnostics(&comments.diagnostics)?;
    check_diagnostics(&project.template_index().diagnostics)?;
    for manuscript in project.manuscript_indices().values() {
        check_diagnostics(&manuscript.diagnostics)?;
    }
    check_diagnostics(&project.saved_query_index().diagnostics)?;
    for profile in project.reader_profiles()? {
        crate::reader_export::plan::validate_selection(&profile.selection)?;
    }
    for (locale, path) in &registry.localizations {
        validate_localization(
            locale,
            &parse_unique_json(project.authoring_document(path)?.bytes())
                .map_err(|error| error.to_string())?,
        )?;
    }
    Ok(comments)
}

fn check_diagnostics(diagnostics: &[Diagnostic]) -> Result<(), String> {
    if let Some(error) = diagnostics
        .iter()
        .find(|item| item.severity == Severity::Error)
    {
        return Err(format!(
            "已登记文档检查失败：{} {}",
            error.code, error.message
        ));
    }
    Ok(())
}

fn validate_localization(locale: &str, value: &Value) -> Result<(), String> {
    let source = value
        .get("source_locale")
        .and_then(Value::as_str)
        .ok_or("本地化缺少 source_locale")?;
    if !crate::workspace_documents::valid_id(source)
        || !crate::workspace_documents::valid_id(locale)
        || source == locale
        || value.get("target_locale").and_then(Value::as_str) != Some(locale)
        || !value
            .get("required_features")
            .and_then(Value::as_array)
            .is_some_and(|features| {
                features.iter().any(|feature| {
                    feature.as_str() == Some(crate::localization::LOCALIZATION_REQUIRED_FEATURE)
                })
            })
    {
        return Err("本地化 locale 或必需能力无效，无法安全移动源码".into());
    }
    let entries = value
        .get("entries")
        .and_then(Value::as_object)
        .ok_or("本地化 entries 必须是对象")?;
    for entry in entries.values() {
        if entry
            .get("source_revision")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err("本地化条目缺少 source_revision".into());
        }
        let parts = entry
            .get("translation_parts")
            .ok_or("本地化条目缺少 translation_parts")?;
        serde_json::from_value::<Option<Vec<crate::localization::LocalizationPart>>>(parts.clone())
            .map_err(|error| format!("本地化译文结构无效：{error}"))?;
    }
    Ok(())
}

fn registered(paths: &BTreeMap<String, PathBuf>, path: &Path) -> bool {
    paths.values().any(|registered| registered == path)
}

fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn inventory(
    project: &Project,
    registry: &Registry,
    path: &Path,
    value: &Value,
) -> Result<Inventory, String> {
    let mut slots = Inventory::new();
    if path == manifest_path(&project.root) {
        for key in ["active", "archived"] {
            relative_array(project, value, &format!("/source_config/{key}"), &mut slots)?;
        }
    } else if registered(&registry.maps, path) {
        for prefix in ["/placements", "/scene/nodes"] {
            if let Some(nodes) = value.pointer(prefix).and_then(Value::as_object) {
                for key in nodes.keys() {
                    let base = pointer(prefix, key);
                    reference(project, value, &format!("{base}/target_ref"), &mut slots)?;
                    reference_array(project, value, &format!("{base}/scope_refs"), &mut slots)?;
                }
            }
        }
    } else if registered(&registry.graph_views, path) {
        reference(project, value, "/focus", &mut slots)?;
        if let Some(positions) = value.get("positions").and_then(Value::as_object) {
            for key in positions.keys() {
                if let Some(id) = key.strip_prefix("file:") {
                    add_slot(
                        project,
                        &pointer("/positions", key),
                        id,
                        PathKind::PositionKey,
                        &mut slots,
                    )?;
                }
            }
        }
    } else if registered(&registry.comments, path) {
        match value.pointer("/anchor/kind").and_then(Value::as_str) {
            Some("object") => reference(project, value, "/anchor/target", &mut slots)?,
            Some("text_range") => {
                let relative = value
                    .pointer("/anchor/path")
                    .and_then(Value::as_str)
                    .ok_or("批注 path 无效")?;
                if relative.contains(':')
                    || relative
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
                {
                    return Err("正文批注 path 必须是规范工作区相对路径".into());
                }
                add_slot(
                    project,
                    "/anchor/path",
                    relative,
                    PathKind::Relative,
                    &mut slots,
                )?;
            }
            _ => {}
        }
    } else if registered(&registry.presets, path) {
        reference_array(project, value, "/scope_refs", &mut slots)?;
    } else if registered(&registry.saved_queries, path) {
        if let Some(filters) = value.pointer("/query/filters").and_then(Value::as_array) {
            for (index, filter) in filters.iter().enumerate() {
                let base = format!("/query/filters/{index}");
                match filter.get("dimension").and_then(Value::as_str) {
                    Some("author_scope") => {
                        relative_array(project, value, &format!("{base}/source_files"), &mut slots)?
                    }
                    Some("relation") => {
                        if let Some(conditions) = filter.get("values").and_then(Value::as_array) {
                            for index in 0..conditions.len() {
                                reference(
                                    project,
                                    value,
                                    &format!("{base}/values/{index}/related"),
                                    &mut slots,
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    } else if registered(&registry.reader_profiles, path) {
        reference_array(project, value, "/selection/objects", &mut slots)?;
        for prefix in ["/selection/fields", "/routes"] {
            if let Some(items) = value.pointer(prefix).and_then(Value::as_array) {
                for index in 0..items.len() {
                    reference(
                        project,
                        value,
                        &format!("{prefix}/{index}/target"),
                        &mut slots,
                    )?;
                }
            }
        }
    }
    Ok(slots)
}

fn reference(
    project: &Project,
    value: &Value,
    at: &str,
    slots: &mut Inventory,
) -> Result<(), String> {
    if let Some(target) = value
        .pointer(at)
        .filter(|target| target.get("kind").and_then(Value::as_str) == Some("file"))
    {
        let id = target
            .get("id")
            .and_then(Value::as_str)
            .ok_or("file TargetRef 缺少字符串 id")?;
        add_slot(project, &format!("{at}/id"), id, PathKind::Identity, slots)?;
    }
    Ok(())
}

fn reference_array(
    project: &Project,
    value: &Value,
    at: &str,
    slots: &mut Inventory,
) -> Result<(), String> {
    if let Some(items) = value.pointer(at).and_then(Value::as_array) {
        for index in 0..items.len() {
            reference(project, value, &format!("{at}/{index}"), slots)?;
        }
    }
    Ok(())
}

fn relative_array(
    project: &Project,
    value: &Value,
    at: &str,
    slots: &mut Inventory,
) -> Result<(), String> {
    if let Some(items) = value.pointer(at).and_then(Value::as_array) {
        for (index, item) in items.iter().enumerate() {
            let relative = item.as_str().ok_or("已登记源码路径必须是字符串")?;
            add_slot(
                project,
                &format!("{at}/{index}"),
                relative,
                PathKind::Relative,
                slots,
            )?;
        }
    }
    Ok(())
}

fn add_slot(
    project: &Project,
    at: &str,
    source: &str,
    kind: PathKind,
    slots: &mut Inventory,
) -> Result<(), String> {
    let path = Path::new(source);
    if source.is_empty() || path.is_absolute() == (kind == PathKind::Relative) {
        return Err(format!("已登记源码路径表示不受支持：{at}"));
    }
    let target = if kind == PathKind::Relative {
        project.root.join(path)
    } else {
        path.to_path_buf()
    };
    let target = crate::file_access::within(&project.root, &target)?;
    if target.extension().and_then(|extension| extension.to_str()) != Some("wl")
        || (kind != PathKind::Relative && target != path)
    {
        return Err(format!("已登记 file 身份必须是工作区规范 .wl 路径：{at}"));
    }
    if slots.len() >= 16384 {
        return Err("已登记文档正式路径超过 16384 项预算，工程未修改".into());
    }
    slots.insert(at.into(), PathSlot { target, kind });
    Ok(())
}

fn verify_inventory(
    project: &Project,
    registry: &Registry,
    path: &Path,
    before: &Inventory,
    after: &Value,
    old: &Path,
    new: &Path,
) -> Result<(), String> {
    let mut expected = Inventory::new();
    for (at, slot) in before {
        let mut slot = slot.clone();
        let mut at = at.clone();
        if slot.target == old {
            slot.target = new.to_path_buf();
            if slot.kind == PathKind::PositionKey {
                at = pointer("/positions", &format!("file:{}", new.display()));
            }
        }
        if expected.insert(at, slot).is_some() {
            return Err("源码移动将合并已登记的路径身份，拒绝覆盖".into());
        }
    }
    if expected != inventory(project, registry, path, after)? {
        return Err(format!(
            "已登记路径语义清单在迁移后不等价：{}",
            path.display()
        ));
    }
    Ok(())
}
