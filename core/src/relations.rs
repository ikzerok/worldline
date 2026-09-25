//! 语言 1.10 的独立语义关系、旧人物关系投影与局部邻接查询。
//!
//! 关系只属于作者资料目录：它们不生成执行图、不改变运行状态，也不进入
//! `fingerprint_program`。旧 `character` 块中的 `relation` 仍由旧语法解析，
//! 这里仅为它生成可读的临时句柄。

use crate::ast::{Program, Property, PropertyValue, RelationDef, RelationTypeDecl};
use crate::catalog::{Catalog, TargetRef};
use crate::diagnostic::{Diagnostic, Span};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// 关系类型的方向。`Undirected` 只影响读取投影；源码仍只保存一条关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationDirection {
    #[default]
    Directed,
    Undirected,
}

/// 目录中的稳定关系类型身份与显示资料。
#[derive(Debug, Clone, Serialize)]
pub struct RelationTypeInfo {
    pub id: String,
    pub display: String,
    pub inverse_display: Option<String>,
    pub direction: RelationDirection,
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
    pub file: String,
    pub line: u32,
}

/// 目录中的独立关系实例。
#[derive(Debug, Clone, Serialize)]
pub struct SemanticRelationInfo {
    pub id: String,
    pub relation_type: String,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub description: String,
    pub source_note: Option<String>,
    pub scope_refs: Vec<TargetRef>,
    pub properties: BTreeMap<String, PropertyValue>,
    pub file: String,
    pub line: u32,
}

/// 旧 `CharacterRelation` 的临时显示身份。
///
/// `occurrence` 仅用于在同一源码对象的重复关系中区分行；它不是可持久化
/// 的关系 ID，也不能写入地图、批注或共享展示文档。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LegacyRelationHandle {
    pub source: TargetRef,
    pub target: TargetRef,
    pub label: String,
    pub occurrence: u32,
    pub file: String,
    pub line: u32,
}

impl LegacyRelationHandle {
    pub fn is_persistent(&self) -> bool {
        false
    }

    pub fn stable_id(&self) -> Option<&str> {
        None
    }
}

/// 旧人物关系的兼容投影。`handle` 的字段会随着源码编辑重新计算。
#[derive(Debug, Clone, Serialize)]
pub struct LegacyRelationInfo {
    pub handle: LegacyRelationHandle,
}

/// 邻接查询方向筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationQueryDirection {
    Outgoing,
    Incoming,
    #[default]
    Both,
}

/// 受限局部关系查询选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationQueryOptions {
    /// 同一快照和筛选下，跳过确定性遍历已经返回的关系数。
    #[serde(default)]
    pub offset: usize,
    /// 只允许 1 或 2；0 会被规范化为 1。
    pub depth: u8,
    pub relation_type: Option<String>,
    /// 多类型 OR 筛选；与单类型条件及方向条件取 AND。
    #[serde(default)]
    pub relation_types: Vec<String>,
    pub direction: RelationQueryDirection,
    pub max_nodes: usize,
    pub max_edges: usize,
}

impl Default for RelationQueryOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            depth: 1,
            relation_type: None,
            relation_types: Vec::new(),
            direction: RelationQueryDirection::Both,
            max_nodes: 250,
            max_edges: 500,
        }
    }
}

impl RelationQueryOptions {
    pub fn bounded(mut self) -> Self {
        self.depth = self.depth.clamp(1, 2);
        // 起点始终是结果的一部分；即使调用方传入 0，也要返回一个封闭结果。
        self.max_nodes = self.max_nodes.clamp(1, 250);
        self.max_edges = self.max_edges.min(500);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryNode {
    #[serde(rename = "ref")]
    pub target: TargetRef,
    pub depth: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryEdge {
    pub id: String,
    pub relation_type: String,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub label: String,
    pub direction: RelationDirection,
    pub source_note: Option<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryContinuation {
    pub offset: usize,
    pub target: TargetRef,
    pub depth: u8,
    pub relation_type: Option<String>,
    /// 多类型 OR 筛选；与单类型条件及方向条件取 AND。
    #[serde(default)]
    pub relation_types: Vec<String>,
    pub direction: RelationQueryDirection,
    /// 因上限未展开的实际边界对象；调用方可从这些对象继续请求更窄结果。
    pub frontier: Vec<TargetRef>,
}

/// 查询结果带明确版本和截断状态，供 CLI/RPC/编辑器直接消费。
#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryResult {
    pub schema_version: u32,
    pub target: TargetRef,
    pub depth: u8,
    pub nodes: Vec<RelationQueryNode>,
    pub edges: Vec<RelationQueryEdge>,
    pub truncated: bool,
    pub continuation: Option<RelationQueryContinuation>,
}

/// 关系类型的结构编辑输入。
#[derive(Debug, Clone, Default)]
pub struct RelationTypeDraft {
    pub id: String,
    pub display: String,
    pub inverse_display: Option<String>,
    pub direction: RelationDirection,
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
}

/// 关系实例的结构编辑输入。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RelationDraft {
    pub id: String,
    pub relation_type: String,
    pub from: TargetRef,
    pub to: TargetRef,
    pub description: String,
    pub source_note: Option<String>,
    pub scope_refs: Vec<TargetRef>,
    pub properties: Vec<(String, PropertyValue)>,
}

/// 旧人物关系提升时使用的简化资料输入。
#[derive(Debug, Clone, Default)]
pub struct LegacyRelationPromotionDraft {
    pub relation_id: String,
    pub relation_type: String,
    pub description: String,
    pub source_note: Option<String>,
}

/// 关系提升的预览结果。预览期间不改 Project。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelationPromotionPreview {
    pub handle: LegacyRelationHandle,
    /// 预览时的内容基线；提交必须与当前工程完全一致。
    pub content_baseline: String,
    /// 预览使用的完整关系草稿，包含 scope 与 properties。
    pub draft: RelationDraft,
    pub relation_id: String,
    pub relation_type: String,
    pub description: String,
    pub source_note: Option<String>,
    pub before_fingerprint: u64,
    pub after_fingerprint: u64,
    pub fingerprint_changed: bool,
}

impl RelationPromotionPreview {
    fn draft(&self) -> RelationDraft {
        self.draft.clone()
    }
}

/// 从 AST 建立关系目录并产生关系诊断。
pub(crate) fn collect(program: &Program, catalog: &mut Catalog, diags: &mut Vec<Diagnostic>) {
    let mut known_objects: HashSet<TargetRef> = catalog
        .objects
        .iter()
        .map(|object| object.target.clone())
        .collect();
    let mut relation_types: BTreeMap<String, RelationTypeInfo> = BTreeMap::new();
    for declaration in &program.relation_types {
        if declaration.name.is_empty() {
            continue;
        }
        if let Some(old) = relation_types.get(&declaration.name) {
            diags.push(
                Diagnostic::error(
                    "A220",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.name.chars().count() as u32,
                    ),
                    format!("关系类型 `{}` 重复定义", declaration.name),
                )
                .with_related(
                    &old.file,
                    Span::new(old.line, 1, declaration.name.chars().count() as u32),
                ),
            );
            continue;
        }
        for (side, kind) in [
            ("from", declaration.from_kind.as_deref()),
            ("to", declaration.to_kind.as_deref()),
        ] {
            if let Some(kind) = kind {
                if !crate::catalog::TARGET_KINDS.contains(&kind) {
                    diags.push(Diagnostic::error(
                        "A222",
                        &declaration.file,
                        Span::new(
                            declaration.loc.line,
                            declaration.loc.column,
                            declaration.name.len() as u32,
                        ),
                        format!(
                            "关系类型 `{}` 的 {side} 端点类型 `{kind}` 无效",
                            declaration.name
                        ),
                    ));
                }
            }
        }
        relation_types.insert(declaration.name.clone(), type_info(declaration));
    }
    catalog.relation_types = relation_types;

    // 关系实例本身也是完整的 TargetRef；先登记全部 ID，允许声明顺序之外的
    // 关系范围或关系端点引用，并由重复 ID 诊断决定最终目录条目。
    known_objects.extend(
        program
            .relations
            .iter()
            .filter(|declaration| !declaration.id.is_empty())
            .map(|declaration| TargetRef::new("relation", &declaration.id)),
    );
    let mut relations: BTreeMap<String, SemanticRelationInfo> = BTreeMap::new();
    for declaration in &program.relations {
        if declaration.id.is_empty() {
            continue;
        }
        if let Some(old) = relations.get(&declaration.id) {
            diags.push(
                Diagnostic::error(
                    "A223",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.id.chars().count() as u32,
                    ),
                    format!("关系 `{}` 重复定义", declaration.id),
                )
                .with_related(
                    &old.file,
                    Span::new(old.line, 1, declaration.id.chars().count() as u32),
                ),
            );
            continue;
        }
        let type_info = catalog.relation_types.get(&declaration.relation_type);
        if type_info.is_none() {
            diags.push(Diagnostic::error(
                "A221",
                &declaration.file,
                Span::new(
                    declaration.loc.line,
                    declaration.loc.column,
                    declaration.id.len() as u32,
                ),
                format!(
                    "关系 `{}` 引用了未定义的关系类型 `{}`",
                    declaration.id, declaration.relation_type
                ),
            ));
        }
        validate_endpoint(
            &declaration.from,
            type_info.and_then(|info| info.from_kind.as_deref()),
            "from",
            declaration,
            &known_objects,
            diags,
        );
        validate_endpoint(
            &declaration.to,
            type_info.and_then(|info| info.to_kind.as_deref()),
            "to",
            declaration,
            &known_objects,
            diags,
        );
        for scope in &declaration.scope_refs {
            if !known_objects.contains(scope) {
                diags.push(Diagnostic::error(
                    "A222",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.id.len() as u32,
                    ),
                    format!(
                        "关系 `{}` 的 scope 引用了不存在的对象 {}:{}",
                        declaration.id, scope.kind, scope.id
                    ),
                ));
            }
        }
        let properties = properties(&declaration.properties, &declaration.file, diags);
        let info = SemanticRelationInfo {
            id: declaration.id.clone(),
            relation_type: declaration.relation_type.clone(),
            from_ref: declaration.from.clone(),
            to_ref: declaration.to.clone(),
            description: declaration.description.clone(),
            source_note: declaration.source_note.clone(),
            scope_refs: declaration.scope_refs.clone(),
            properties,
            file: declaration.file.clone(),
            line: declaration.loc.line,
        };
        catalog.add_object(
            "relation",
            &declaration.id,
            if declaration.description.is_empty() {
                &declaration.id
            } else {
                &declaration.description
            },
            &declaration.file,
            declaration.loc.line,
        );
        known_objects.insert(TargetRef::new("relation", &declaration.id));
        relations.insert(declaration.id.clone(), info);
    }
    catalog.relations = relations;
    catalog.relation_index = relation_index(catalog);

    // 关系端点是内容引用；它们必须参加删除影响检查。
    for relation in catalog.relations.values() {
        catalog.references.push(crate::catalog::ReferenceInfo {
            source: TargetRef::new("relation", &relation.id),
            target: relation.from_ref.clone(),
            kind: "语义关系 from 端点".into(),
            file: relation.file.clone(),
            line: relation.line,
        });
        catalog.references.push(crate::catalog::ReferenceInfo {
            source: TargetRef::new("relation", &relation.id),
            target: relation.to_ref.clone(),
            kind: "语义关系 to 端点".into(),
            file: relation.file.clone(),
            line: relation.line,
        });
        for scope in &relation.scope_refs {
            catalog.references.push(crate::catalog::ReferenceInfo {
                source: TargetRef::new("relation", &relation.id),
                target: scope.clone(),
                kind: "语义关系范围".into(),
                file: relation.file.clone(),
                line: relation.line,
            });
        }
    }

    // 旧关系始终保留旧语义和指纹，只追加只读投影。重复项按出现次序区分。
    let mut legacy = Vec::new();
    for character in &program.characters {
        let source = TargetRef::new("character", &character.name);
        let mut occurrences: HashMap<(String, String), u32> = HashMap::new();
        for relation in &character.relations {
            let key = (relation.target.clone(), relation.label.clone());
            let occurrence = occurrences.entry(key).and_modify(|n| *n += 1).or_insert(1);
            legacy.push(LegacyRelationInfo {
                handle: LegacyRelationHandle {
                    source: source.clone(),
                    target: TargetRef::new("character", &relation.target),
                    label: relation.label.clone(),
                    occurrence: *occurrence,
                    file: character.file.clone(),
                    line: relation.loc.line,
                },
            });
        }
    }
    catalog.legacy_relations = legacy;
}

fn type_info(declaration: &RelationTypeDecl) -> RelationTypeInfo {
    RelationTypeInfo {
        id: declaration.name.clone(),
        display: declaration
            .display
            .clone()
            .unwrap_or_else(|| declaration.name.clone()),
        inverse_display: declaration.inverse_display.clone(),
        direction: declaration.direction,
        from_kind: declaration.from_kind.clone(),
        to_kind: declaration.to_kind.clone(),
        file: declaration.file.clone(),
        line: declaration.loc.line,
    }
}

fn properties(
    items: &[Property],
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> BTreeMap<String, PropertyValue> {
    let mut result = BTreeMap::new();
    for property in items {
        if result
            .insert(property.name.clone(), property.value.clone())
            .is_some()
        {
            diags.push(Diagnostic::error(
                "A220",
                file,
                Span::new(
                    property.loc.line,
                    property.loc.column,
                    property.name.len() as u32,
                ),
                format!("关系属性 `{}` 重复定义", property.name),
            ));
        }
    }
    result
}

fn validate_endpoint(
    endpoint: &TargetRef,
    expected_kind: Option<&str>,
    side: &str,
    declaration: &RelationDef,
    known_objects: &HashSet<TargetRef>,
    diags: &mut Vec<Diagnostic>,
) {
    if !crate::catalog::TARGET_KINDS.contains(&endpoint.kind.as_str()) {
        diags.push(Diagnostic::error(
            "A222",
            &declaration.file,
            Span::new(
                declaration.loc.line,
                declaration.loc.column,
                declaration.id.len() as u32,
            ),
            format!(
                "关系 `{}` 的 {side} 端点类型 `{}` 无效",
                declaration.id, endpoint.kind
            ),
        ));
    } else if !known_objects.contains(endpoint) {
        diags.push(Diagnostic::error(
            "A222",
            &declaration.file,
            Span::new(
                declaration.loc.line,
                declaration.loc.column,
                declaration.id.len() as u32,
            ),
            format!(
                "关系 `{}` 的 {side} 端点 {}:{} 不存在",
                declaration.id, endpoint.kind, endpoint.id
            ),
        ));
    }
    if let Some(expected) = expected_kind {
        if endpoint.kind != expected {
            diags.push(Diagnostic::error(
                "A222",
                &declaration.file,
                Span::new(
                    declaration.loc.line,
                    declaration.loc.column,
                    declaration.id.len() as u32,
                ),
                format!(
                    "关系 `{}` 的 {side} 端点类型应为 `{expected}`,实际为 `{}`",
                    declaration.id, endpoint.kind
                ),
            ));
        }
    }
}

impl Catalog {
    /// 在同一目录快照上读取下一页，沿用前页的目标和筛选。
    pub fn continue_relations(
        &self,
        continuation: &RelationQueryContinuation,
    ) -> RelationQueryResult {
        self.query_relations(
            &continuation.target,
            RelationQueryOptions {
                offset: continuation.offset,
                depth: continuation.depth,
                relation_type: continuation.relation_type.clone(),
                relation_types: continuation.relation_types.clone(),
                direction: continuation.direction,
                ..Default::default()
            },
        )
    }

    /// 查询独立关系；不生成反向关系、传递关系或推断路径。
    pub fn query_relations(
        &self,
        target: &TargetRef,
        options: RelationQueryOptions,
    ) -> RelationQueryResult {
        let options = options.bounded();
        let mut nodes = Vec::new();
        let mut node_depth = BTreeMap::<TargetRef, u8>::from([(target.clone(), 0)]);
        let mut discovered = BTreeMap::<TargetRef, u8>::from([(target.clone(), 0)]);
        let mut queue = VecDeque::from([(target.clone(), 0u8)]);
        let mut edges = Vec::new();
        let mut emitted = BTreeSet::new();
        let mut frontier = BTreeSet::new();
        let mut truncated = false;
        let mut position = 0usize;

        'traverse: while let Some((current, depth)) = queue.pop_front() {
            if depth >= options.depth {
                continue;
            }
            let candidates = self
                .relation_index
                .get(&current)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| self.relations.get(id))
                .filter(|relation| {
                    let undirected = self
                        .relation_types
                        .get(&relation.relation_type)
                        .is_some_and(|info| info.direction == RelationDirection::Undirected);
                    options
                        .relation_type
                        .as_deref()
                        .is_none_or(|kind| relation.relation_type == kind)
                        && (options.relation_types.is_empty()
                            || options.relation_types.contains(&relation.relation_type))
                        && (undirected
                            || ((relation.from_ref == current
                                && options.direction != RelationQueryDirection::Incoming)
                                || (relation.to_ref == current
                                    && options.direction != RelationQueryDirection::Outgoing)))
                });
            for relation in candidates {
                if !emitted.insert(relation.id.clone()) {
                    continue;
                }
                let (next, reverse) = if relation.from_ref == current {
                    (&relation.to_ref, false)
                } else {
                    (&relation.from_ref, true)
                };
                let next_depth = depth + 1;
                // 翻页跳过显示时仍沿原快照遍历，避免遗漏第二层的连接。
                if !discovered.contains_key(next) {
                    discovered.insert(next.clone(), next_depth);
                    queue.push_back((next.clone(), next_depth));
                }
                if position < options.offset {
                    position += 1;
                    continue;
                }
                let additional = usize::from(!node_depth.contains_key(&current))
                    + usize::from(next != &current && !node_depth.contains_key(next));
                if edges.len() >= options.max_edges
                    || node_depth.len() + additional > options.max_nodes
                {
                    truncated = true;
                    frontier.insert(current.clone());
                    frontier.insert(next.clone());
                    break 'traverse;
                }
                node_depth.insert(current.clone(), discovered[&current]);
                node_depth.insert(next.clone(), discovered[next]);
                let type_info = self.relation_types.get(&relation.relation_type);
                let label = if reverse {
                    type_info
                        .and_then(|info| info.inverse_display.clone())
                        .unwrap_or_else(|| {
                            type_info
                                .map(|info| info.display.clone())
                                .unwrap_or_else(|| relation.relation_type.clone())
                        })
                } else {
                    type_info
                        .map(|info| info.display.clone())
                        .unwrap_or_else(|| relation.relation_type.clone())
                };
                edges.push(RelationQueryEdge {
                    id: relation.id.clone(),
                    relation_type: relation.relation_type.clone(),
                    from_ref: relation.from_ref.clone(),
                    to_ref: relation.to_ref.clone(),
                    label,
                    direction: type_info
                        .map(|info| info.direction)
                        .unwrap_or(RelationDirection::Directed),
                    source_note: relation.source_note.clone(),
                    file: relation.file.clone(),
                    line: relation.line,
                });
                position += 1;
            }
        }
        for (node, depth) in node_depth {
            nodes.push(RelationQueryNode {
                target: node,
                depth,
            });
        }
        nodes.sort_by(|a, b| (a.depth, &a.target).cmp(&(b.depth, &b.target)));
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        RelationQueryResult {
            schema_version: 1,
            target: target.clone(),
            depth: options.depth,
            nodes,
            edges,
            truncated,
            continuation: truncated.then(|| RelationQueryContinuation {
                offset: position,
                target: target.clone(),
                depth: options.depth,
                relation_type: options.relation_type.clone(),
                relation_types: options.relation_types.clone(),
                direction: options.direction,
                frontier: frontier.into_iter().collect(),
            }),
        }
    }

    pub fn legacy_relation_handles(&self) -> Vec<LegacyRelationHandle> {
        self.legacy_relations
            .iter()
            .map(|relation| relation.handle.clone())
            .collect()
    }
}

/// 根据关系类型与实例构建机器可读的关系索引（供未来快照 DTO 复用）。
fn relation_index(catalog: &Catalog) -> BTreeMap<TargetRef, Vec<String>> {
    let mut index = BTreeMap::new();
    for relation in catalog.relations.values() {
        index
            .entry(relation.from_ref.clone())
            .or_insert_with(Vec::new)
            .push(relation.id.clone());
        index
            .entry(relation.to_ref.clone())
            .or_insert_with(Vec::new)
            .push(relation.id.clone());
    }
    for ids in index.values_mut() {
        ids.sort();
    }
    index
}

// ---------------------------------------------------------------------------
// Project 结构编辑与旧关系提升
// ---------------------------------------------------------------------------

impl crate::project::Project {
    fn ensure_relation_capability(&self) -> Result<(), String> {
        if !self.language_version_kind().supports_relations() {
            return Err("关系编辑需要显式语言版本 1.10".into());
        }
        let path = crate::workspace_documents::manifest_path(&self.root);
        let document = self.authoring_document(&path)?;
        let manifest: serde_json::Value = serde_json::from_slice(document.bytes())
            .map_err(|error| format!("无法读取关系能力清单：{error}"))?;
        if document.is_deleted()
            || !manifest
                .get("required_features")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|features| {
                    features
                        .iter()
                        .any(|feature| feature.as_str() == Some("content.relations.v1"))
                })
        {
            return Err("关系编辑需要清单声明 content.relations.v1".into());
        }
        Ok(())
    }

    /// 创建或修改关系类型。调用方应在 `Project::edit` 外直接使用本方法；
    /// 方法自身沿用 clone→compile→提交的整批事务边界。
    pub fn write_relation_type(
        &mut self,
        original: Option<&str>,
        draft: &RelationTypeDraft,
    ) -> Result<(), String> {
        let draft = draft.clone();
        let original = original.map(str::to_string);
        self.edit(move |candidate| candidate.write_relation_type_raw(original.as_deref(), &draft))
    }

    fn write_relation_type_raw(
        &mut self,
        original: Option<&str>,
        draft: &RelationTypeDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        crate::authoring::identifier(&draft.id)?;
        if draft.display.trim().is_empty() {
            return Err("关系类型显示名不能为空".into());
        }
        if let Some(kind) = &draft.from_kind {
            validate_relation_kind(kind)?;
        }
        if let Some(kind) = &draft.to_kind {
            validate_relation_kind(kind)?;
        }
        if original.is_some_and(|id| id != draft.id) {
            return Err("关系类型 ID 是引用身份,修改资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let existing = result.analysis.catalog.relation_types.get(&draft.id);
        if original.is_some() && existing.is_none() {
            return Err("待修改的关系类型不存在".into());
        }
        if original.is_none() && existing.is_some() {
            return Err("关系类型 ID 已存在".into());
        }
        let path = existing
            .map(|info| PathBuf::from(&info.file))
            .unwrap_or_else(|| self.entry.clone());
        let source = relation_type_source(draft)?;
        if let Some(info) = existing {
            replace_relation_block(self, &path, info.line, true, &source)
        } else {
            append_relation_source(self, &path, &source)
        }
    }

    /// 创建或修改一个独立关系实例。
    pub fn write_relation(
        &mut self,
        original: Option<&str>,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        let draft = draft.clone();
        let original = original.map(str::to_string);
        self.edit(move |candidate| candidate.write_relation_raw(original.as_deref(), &draft))
    }

    fn write_relation_raw(
        &mut self,
        original: Option<&str>,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        crate::authoring::identifier(&draft.id)?;
        crate::authoring::identifier(&draft.relation_type)?;
        validate_relation_target(&draft.from, self.compile_options())?;
        validate_relation_target(&draft.to, self.compile_options())?;
        for reference in &draft.scope_refs {
            validate_relation_target(reference, self.compile_options())?;
        }
        if original.is_some_and(|id| id != draft.id) {
            return Err("关系 ID 是引用身份,修改资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let existing = result.analysis.catalog.relations.get(&draft.id);
        if original.is_some() && existing.is_none() {
            return Err("待修改的关系不存在".into());
        }
        if original.is_none() && existing.is_some() {
            return Err("关系 ID 已存在".into());
        }
        if !result
            .analysis
            .catalog
            .relation_types
            .contains_key(&draft.relation_type)
        {
            return Err(format!("关系类型 `{}` 不存在", draft.relation_type));
        }
        let path = existing
            .map(|info| PathBuf::from(&info.file))
            .unwrap_or_else(|| self.entry.clone());
        for target in std::iter::once(&draft.from)
            .chain(std::iter::once(&draft.to))
            .chain(draft.scope_refs.iter())
        {
            validate_relation_file_target(&self.root, target)?;
        }
        let source = relation_source(draft, &path)?;
        if let Some(info) = existing {
            replace_relation_block(self, &path, info.line, false, &source)
        } else {
            append_relation_source(self, &path, &source)
        }
    }

    /// 删除关系；地图/视图等显式关系引用会沿既有删除影响边界阻止提交。
    pub fn remove_relation(&mut self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.edit(move |candidate| candidate.remove_relation_raw(&id))
    }

    fn remove_relation_raw(&mut self, id: &str) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let result = self.compile_current();
        let relation = result
            .analysis
            .catalog
            .relations
            .get(id)
            .cloned()
            .ok_or("关系不存在")?;
        let impact = self.deletion_impact(&TargetRef::new("relation", id));
        if !impact.complete {
            return Err("引用检查不完整，请先修复内容或地图诊断，再删除关系".into());
        }
        if !impact.content_references.is_empty() {
            return Err(format!("关系 `{id}` 仍被正文或目录引用，请先解除这些引用"));
        }
        if !impact.map_placements.is_empty()
            || !impact.map_scopes.is_empty()
            || !impact.graph_views.is_empty()
        {
            return Err(format!("关系 `{id}` 仍被展示文档引用，请先解除这些引用"));
        }
        replace_relation_block(
            self,
            &PathBuf::from(&relation.file),
            relation.line,
            false,
            "",
        )
    }

    pub fn remove_relation_type(&mut self, id: &str) -> Result<(), String> {
        let id = id.to_string();
        self.edit(move |candidate| candidate.remove_relation_type_raw(&id))
    }

    fn remove_relation_type_raw(&mut self, id: &str) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let result = self.compile_current();
        let relation_type = result
            .analysis
            .catalog
            .relation_types
            .get(id)
            .cloned()
            .ok_or("关系类型不存在")?;
        if result
            .analysis
            .catalog
            .relations
            .values()
            .any(|relation| relation.relation_type == id)
        {
            return Err(format!(
                "关系类型 `{id}` 仍被关系实例使用，请先删除或修改这些关系"
            ));
        }
        let views = crate::graph_views::build_graph_view_index(self, &result);
        if views
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err("网络视图引用检查不完整，不能删除关系类型".into());
        }
        let referencing_views = views
            .views
            .values()
            .filter(|view| {
                view.draft
                    .filters
                    .relation_types
                    .iter()
                    .any(|reference| reference == id)
            })
            .map(|view| view.draft.id.clone())
            .collect::<Vec<_>>();
        if !referencing_views.is_empty() {
            return Err(format!(
                "关系类型 `{id}` 仍被共享视图筛选引用，请先修改筛选：{}",
                referencing_views.join("、")
            ));
        }
        replace_relation_block(
            self,
            &PathBuf::from(&relation_type.file),
            relation_type.line,
            true,
            "",
        )
    }

    /// 预览旧人物关系提升；此调用不修改 Project。
    pub fn preview_promote_legacy_relation(
        &self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<RelationPromotionPreview, String> {
        if draft.from != handle.source || draft.to != handle.target {
            return Err("提升关系的 from/to 必须与旧人物关系一致".into());
        }
        let current = self.compile_current();
        if !current
            .analysis
            .catalog
            .legacy_relation_handles()
            .contains(handle)
        {
            return Err("旧人物关系句柄已失效，请重新读取工程".into());
        }
        let before = current.analysis.fingerprint;
        let mut candidate = self.clone();
        candidate.apply_legacy_promotion_raw(handle, draft)?;
        let compiled = candidate.compile();
        if let Some(error) = compiled
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
        {
            return Err(format!("{} {}", error.code, error.message));
        }
        let after = compiled.analysis.fingerprint;
        Ok(RelationPromotionPreview {
            handle: handle.clone(),
            content_baseline: self.content_baseline(),
            draft: draft.clone(),
            relation_id: draft.id.clone(),
            relation_type: draft.relation_type.clone(),
            description: draft.description.clone(),
            source_note: draft.source_note.clone(),
            before_fingerprint: before,
            after_fingerprint: after,
            fingerprint_changed: before != after,
        })
    }

    pub fn preview_legacy_relation_promotion(
        &self,
        handle: &LegacyRelationHandle,
        draft: &LegacyRelationPromotionDraft,
    ) -> Result<RelationPromotionPreview, String> {
        let relation = RelationDraft {
            id: draft.relation_id.clone(),
            relation_type: draft.relation_type.clone(),
            from: handle.source.clone(),
            to: handle.target.clone(),
            description: if draft.description.is_empty() {
                handle.label.clone()
            } else {
                draft.description.clone()
            },
            source_note: draft.source_note.clone(),
            ..Default::default()
        };
        self.preview_promote_legacy_relation(handle, &relation)
    }

    /// 按预览提交提升；源文件已变更或句柄已失效时零写入失败。
    pub fn apply_legacy_relation_promotion(
        &mut self,
        preview: &RelationPromotionPreview,
    ) -> Result<(), String> {
        let current = self.compile_current();
        if self.content_baseline() != preview.content_baseline {
            return Err("关系提升的内容基线已过期，请重新预览".into());
        }
        if !current
            .analysis
            .catalog
            .legacy_relation_handles()
            .contains(&preview.handle)
        {
            return Err("旧人物关系句柄已失效，请重新读取工程".into());
        }
        let preview = preview.clone();
        if preview.draft.from != preview.handle.source || preview.draft.to != preview.handle.target
        {
            return Err("提升关系的 from/to 必须与旧人物关系一致".into());
        }
        let verified = self.preview_promote_legacy_relation(&preview.handle, &preview.draft)?;
        if verified != preview {
            return Err("关系提升预览与当前草稿或指纹差异不一致，请重新预览".into());
        }
        self.edit(move |candidate| {
            candidate.apply_legacy_promotion_raw(&preview.handle, &preview.draft())
        })
    }

    pub fn promote_legacy_relation(
        &mut self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<RelationPromotionPreview, String> {
        let preview = self.preview_promote_legacy_relation(handle, draft)?;
        self.apply_legacy_relation_promotion(&preview)?;
        Ok(preview)
    }

    fn apply_legacy_promotion_raw(
        &mut self,
        handle: &LegacyRelationHandle,
        draft: &RelationDraft,
    ) -> Result<(), String> {
        self.ensure_relation_capability()?;
        let path = PathBuf::from(&handle.file);
        let text = self.document(&path)?.to_string();
        let parsed = crate::lexer::lex_source_with_options(
            &path.to_string_lossy(),
            &text,
            &mut Vec::new(),
            self.compile_options(),
        );
        let _line = parsed
            .iter()
            .find(|line| {
                line.no == handle.line
                    && matches!(
                        &line.kind,
                        crate::lexer::LineKind::Relation { target, label, .. }
                            if target == &handle.target.id && label == &handle.label
                    )
            })
            .ok_or("旧人物关系源行不存在")?;
        let mut text = text;
        let start = line_start(&text, handle.line);
        let end = line_end(&text, handle.line);
        let raw = text[start..end].to_string();
        let retained = crate::authoring::comments(&raw);
        text.replace_range(start..end, &retained);
        self.set_text(&path, text)?;
        append_relation_source(self, &path, &relation_source(draft, &path)?)?;
        Ok(())
    }
}

fn validate_relation_kind(kind: &str) -> Result<(), String> {
    if crate::catalog::TARGET_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(format!("关系端点类型 `{kind}` 无效"))
    }
}

fn validate_relation_target(
    target: &TargetRef,
    options: crate::CompileOptions,
) -> Result<(), String> {
    if !crate::catalog::is_target_kind(&target.kind, options) {
        return Err(format!("关系端点类型 `{}` 无效", target.kind));
    }
    if target.kind == "file" {
        if target.id.is_empty() {
            return Err("关系 file TargetRef 路径不能为空".into());
        }
        return Ok(());
    }
    for part in target.id.split('.') {
        crate::authoring::identifier(part)?;
    }
    if target.id.is_empty() {
        return Err("关系端点 ID 不能为空".into());
    }
    Ok(())
}

fn validate_relation_file_target(root: &Path, target: &TargetRef) -> Result<(), String> {
    if target.kind != "file" {
        return Ok(());
    }
    let path = Path::new(&target.id);
    let canonical = crate::compiler::source_path(path);
    if !path.is_absolute() || canonical != path {
        return Err("关系 file TargetRef 必须使用工作区内源码的 canonical 绝对路径".into());
    }
    if !canonical.starts_with(crate::compiler::source_path(root)) {
        return Err("关系 file TargetRef 必须指向当前工作区内的源码".into());
    }
    Ok(())
}

fn relation_type_source(draft: &RelationTypeDraft) -> Result<String, String> {
    let mut out = format!(
        "relation_type {} as {}\n",
        draft.id,
        crate::authoring::quote(&draft.display)
    );
    if let Some(inverse) = &draft.inverse_display {
        out.push_str(&format!("  inverse {}\n", crate::authoring::quote(inverse)));
    }
    out.push_str(&format!(
        "  direction {}\n",
        match draft.direction {
            RelationDirection::Directed => "directed",
            RelationDirection::Undirected => "undirected",
        }
    ));
    if let Some(kind) = &draft.from_kind {
        out.push_str(&format!("  from {kind}\n"));
    }
    if let Some(kind) = &draft.to_kind {
        out.push_str(&format!("  to {kind}\n"));
    }
    Ok(out)
}

fn relation_source(draft: &RelationDraft, declaration_file: &Path) -> Result<String, String> {
    let from = relation_target_source(&draft.from, declaration_file)?;
    let to = relation_target_source(&draft.to, declaration_file)?;
    let mut out = format!(
        "relation_def {} type {} from {} to {}\n",
        draft.id, draft.relation_type, from, to
    );
    if !draft.description.is_empty() {
        out.push_str(&format!(
            "  description {}\n",
            crate::authoring::quote(&draft.description)
        ));
    }
    if let Some(note) = &draft.source_note {
        out.push_str(&format!(
            "  source_note {}\n",
            crate::authoring::quote(note)
        ));
    }
    for scope in &draft.scope_refs {
        out.push_str(&format!(
            "  scope {}\n",
            relation_target_source(scope, declaration_file)?
        ));
    }
    for (name, value) in &draft.properties {
        crate::authoring::identifier(name)?;
        out.push_str(&format!(
            "  property {name} = {}\n",
            crate::authoring::property_source(value)
        ));
    }
    Ok(out)
}

fn relation_target_source(target: &TargetRef, declaration_file: &Path) -> Result<String, String> {
    if target.kind != "file" {
        return Ok(format!("{} {}", target.kind, target.id));
    }
    let parent = declaration_file.parent().unwrap_or(Path::new("."));
    let relative = crate::catalog_edit::relative_source_path(parent, Path::new(&target.id))?;
    Ok(format!("file {}", crate::authoring::quote(&relative)))
}

fn append_relation_source(
    project: &mut crate::project::Project,
    path: &Path,
    source: &str,
) -> Result<(), String> {
    let mut text = project.document(path)?.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(source);
    project.set_text(path, text)
}

fn replace_relation_block(
    project: &mut crate::project::Project,
    path: &Path,
    line_no: u32,
    type_decl: bool,
    source: &str,
) -> Result<(), String> {
    let text = project.document(path)?.to_string();
    let parsed = crate::lexer::lex_source_with_options(
        &path.to_string_lossy(),
        &text,
        &mut Vec::new(),
        project.compile_options(),
    );
    let index = parsed
        .iter()
        .position(|line| {
            line.no == line_no
                && if type_decl {
                    matches!(line.kind, crate::lexer::LineKind::RelationType { .. })
                } else {
                    matches!(line.kind, crate::lexer::LineKind::RelationDef { .. })
                }
        })
        .ok_or("关系声明源位置不存在")?;
    let indent = parsed[index].indent;
    let next = parsed
        .iter()
        .skip(index + 1)
        .find(|line| line.indent <= indent)
        .map(|line| line.no)
        .unwrap_or_else(|| text.lines().count() as u32 + 1);
    let start = line_start(&text, line_no);
    let end = if next <= text.lines().count() as u32 {
        line_start(&text, next)
    } else {
        text.len()
    };
    let retained = crate::authoring::comments(&text[start..end]);
    let mut replacement = retained;
    replacement.push_str(source);
    if !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    let mut text = text;
    text.replace_range(start..end, &replacement);
    project.set_text(path, text)
}

fn line_start(text: &str, line: u32) -> usize {
    text.split_inclusive('\n')
        .take(line.saturating_sub(1) as usize)
        .map(str::len)
        .sum()
}

fn line_end(text: &str, line: u32) -> usize {
    let start = line_start(text, line);
    text[start..]
        .find('\n')
        .map(|offset| start + offset + 1)
        .unwrap_or(text.len())
}
