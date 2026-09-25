//! M4 展示预设：只保存显式展示选择，不创造世界语义。
use crate::catalog::TargetRef;
use crate::graph_views::GraphViewIndex;
use crate::presentation::{MapGeometry, MapIndex};
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use crate::{CompileResult, Diagnostic, Span};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetGeometryRef {
    pub placement_id: String,
    pub purpose: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresentationPresetDraft {
    pub id: String,
    pub title: String,
    pub map_id: Option<String>,
    pub graph_view_id: Option<String>,
    #[serde(default)]
    pub layer_visibility: BTreeMap<String, bool>,
    #[serde(default)]
    pub scope_refs: Vec<TargetRef>,
    #[serde(default)]
    pub include_unscoped: bool,
    #[serde(default)]
    pub include_period_children: bool,
    #[serde(default)]
    pub geometry_refs: Vec<PresetGeometryRef>,
}

#[derive(Clone, Debug)]
pub struct PresentationPresetDocument {
    pub draft: PresentationPresetDraft,
    pub path: PathBuf,
    pub source: Value,
    pub read_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PresentationPresetIndex {
    pub presets: BTreeMap<String, PresentationPresetDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

fn report(
    index: &mut PresentationPresetIndex,
    path: &std::path::Path,
    code: &'static str,
    message: impl Into<String>,
) {
    index.diagnostics.push(Diagnostic::error(
        code,
        &path.to_string_lossy(),
        Span::new(1, 1, 1),
        message,
    ));
}

fn validate(
    draft: &PresentationPresetDraft,
    content: &CompileResult,
    maps: &MapIndex,
    graphs: &GraphViewIndex,
) -> Result<(), String> {
    if !valid_id(&draft.id) || draft.title.trim().is_empty() {
        return Err("预设 ID 无效或标题为空".into());
    }
    if draft.map_id.is_none() && draft.graph_view_id.is_none() {
        return Err("预设至少要引用一张地图或一个网络视图".into());
    }
    if let Some(map_id) = &draft.map_id {
        let map = maps.maps.get(map_id).ok_or("预设引用的地图不存在")?;
        for layer in draft.layer_visibility.keys() {
            if !map.layers.contains_key(layer) {
                return Err(format!("预设引用不存在的图层 `{layer}`"));
            }
        }
        for reference in &draft.geometry_refs {
            let placement = map
                .placements
                .get(&reference.placement_id)
                .ok_or_else(|| format!("预设引用不存在的图元 `{}`", reference.placement_id))?;
            match (reference.purpose.as_str(), &placement.geometry) {
                ("path", MapGeometry::Polyline { .. })
                | ("distribution", MapGeometry::Polygon { .. }) => {}
                ("path" | "distribution", _) => {
                    return Err(format!(
                        "图元 `{}` 的几何与用途不匹配",
                        reference.placement_id
                    ))
                }
                _ => return Err("图元用途只能是 path 或 distribution".into()),
            }
        }
    } else if !draft.layer_visibility.is_empty() || !draft.geometry_refs.is_empty() {
        return Err("没有 map_id 时不能保存图层或路径/分布区引用".into());
    }
    if let Some(view_id) = &draft.graph_view_id {
        if !graphs.views.contains_key(view_id) {
            return Err(format!("预设引用的网络视图 `{view_id}` 不存在"));
        }
    }
    for scope in &draft.scope_refs {
        if content.analysis.catalog.object(scope).is_none() {
            return Err(format!("预设范围对象不存在 {}:{}", scope.kind, scope.id));
        }
    }
    Ok(())
}

pub fn build_preset_index(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
    graphs: &GraphViewIndex,
) -> PresentationPresetIndex {
    let mut index = PresentationPresetIndex::default();
    let manifest = manifest_path(&project.root);
    let Ok(document) = project.authoring_document(&manifest) else {
        return index;
    };
    if document.is_deleted() {
        return index;
    }
    let registry = parse_registry(&project.root, document.bytes());
    index.diagnostics.extend(registry.diagnostics);
    for (id, path) in registry.presets {
        let parsed = (|| -> Result<(PresentationPresetDraft, Value, bool), String> {
            let document = project.authoring_document(&path)?;
            if document.is_deleted() {
                return Err("注册的展示预设已删除".into());
            }
            let source = parse_unique_json(document.bytes())
                .map_err(|e| format!("预设 JSON 无法解析：{e}"))?;
            if source.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("预设 schema_version 不受支持".into());
            }
            let draft: PresentationPresetDraft =
                serde_json::from_value(source.clone()).map_err(|e| format!("预设结构无效：{e}"))?;
            if draft.id != id {
                return Err("预设 ID 与清单注册 ID 不一致".into());
            }
            validate(&draft, content, maps, graphs)?;
            Ok((draft, source, document.is_read_only()))
        })();
        match parsed {
            Ok((draft, source, read_only)) => {
                index.presets.insert(
                    id,
                    PresentationPresetDocument {
                        draft,
                        path,
                        source,
                        read_only,
                    },
                );
            }
            Err(error) => report(&mut index, &path, "PRESET001", error),
        }
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}

#[derive(Clone, Debug)]
pub struct PresetCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub original: Option<String>,
    pub draft: PresentationPresetDraft,
}

#[derive(Clone, Debug)]
pub struct PresetResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    command: PresetCommand,
) -> Result<PresetResult, String> {
    let content = project.compile();
    let maps = crate::presentation_commands::map_index_with_content(project, &content);
    let graphs = crate::graph_views::build_graph_view_index(project, &content);
    apply_with_indexes(project, revision, command, &content, &maps, &graphs)
}

pub fn apply_with_indexes(
    project: &mut Project,
    revision: &mut Revision,
    command: PresetCommand,
    content: &CompileResult,
    maps: &MapIndex,
    graphs: &GraphViewIndex,
) -> Result<PresetResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
        || content.sources != project.sources()
    {
        return Err("StaleRevision：展示预设基线已过期".into());
    }
    validate(&command.draft, content, maps, graphs)?;
    let manifest = manifest_path(&project.root);
    let document = project.authoring_document(&manifest)?;
    if document.is_read_only() || document.is_deleted() {
        return Err("展示预设需要可写的工作区清单".into());
    }
    let registry = parse_registry(&project.root, document.bytes());
    if !registry.diagnostics.is_empty() {
        return Err("工作区清单诊断未修复，不能保存展示预设".into());
    }
    let existing_index = build_preset_index(project, content, maps, graphs);
    let old = match command.original.as_deref() {
        Some(id) if id == command.draft.id => Some(
            existing_index
                .presets
                .get(id)
                .ok_or("待修改的展示预设无法安全读取")?,
        ),
        Some(_) => return Err("展示预设 ID 是稳定身份，不能在保存时改名".into()),
        None => None,
    };
    if old.is_some_and(|preset| preset.read_only) {
        return Err("展示预设为只读，不能覆盖".into());
    }
    if old.is_none() && registry.presets.contains_key(&command.draft.id) {
        return Err("展示预设 ID 已注册，不能覆盖".into());
    }
    let path = old.map(|preset| preset.path.clone()).unwrap_or_else(|| {
        project
            .root
            .join(format!(".world/presets/{}.json", command.draft.id))
    });
    let mut source = old
        .map(|preset| preset.source.clone())
        .unwrap_or_else(|| json!({"schema_version":1}));
    let fresh = serde_json::to_value(&command.draft).map_err(|e| e.to_string())?;
    for (key, value) in fresh.as_object().ok_or("展示预设序列化无效")? {
        source[key] = value.clone();
    }
    let bytes = serde_json::to_vec_pretty(&source).map_err(|e| e.to_string())?;
    let mut candidate = project.clone();
    let mut changed_files = Vec::new();
    if old.is_none() {
        let relative = path
            .strip_prefix(&project.root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let mut manifest_value = parse_unique_json(document.bytes()).map_err(|e| e.to_string())?;
        if manifest_value.get("presets").is_none() {
            manifest_value["presets"] = json!({});
        }
        manifest_value["presets"][&command.draft.id] = json!(relative);
        if manifest_value.get("required_features").is_none() {
            manifest_value["required_features"] = json!([]);
        }
        let features = manifest_value["required_features"]
            .as_array_mut()
            .ok_or("必需能力清单格式无效")?;
        if !features
            .iter()
            .any(|item| item.as_str() == Some("presentation.presets.v1"))
        {
            features.push(json!("presentation.presets.v1"));
        }
        candidate.set_authoring_document(
            &manifest,
            serde_json::to_vec_pretty(&manifest_value).map_err(|e| e.to_string())?,
        )?;
        candidate.create_authoring_document(&path, bytes)?;
        changed_files.push(manifest);
    } else {
        candidate.set_authoring_document(&path, bytes)?;
    }
    changed_files.push(path);
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(PresetResult {
        changed_files,
        new_revision: *revision,
    })
}
