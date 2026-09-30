//! 作者资料目录:稳定对象引用、标签解引用和外部素材检查。
use crate::ast::{Loc, PropertyValue, WorldDecl};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
mod collect;
mod language;
pub(crate) use collect::analyze;

#[derive(
    Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, serde::Deserialize,
)]
pub struct TargetRef {
    pub kind: String,
    pub id: String,
}
impl TargetRef {
    pub fn new(kind: &str, id: &str) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }
}

pub const TARGET_KINDS: &[&str] = &[
    "anchor",
    "state",
    "event",
    "scene",
    "character",
    "entity",
    "world",
    "storyline",
    "period",
    "variable",
    "tag",
    "asset",
    "file",
    "relation",
    "rule",
    "fragment",
];

/// Explicit property references currently target kinds supported by the core rename plan.
pub const OBJECT_REFERENCE_TARGET_KINDS: &[&str] = &["entity", "relation"];

pub fn is_target_kind(kind: &str, options: crate::compiler::CompileOptions) -> bool {
    TARGET_KINDS.contains(&kind)
        && (!matches!(kind, "rule" | "fragment")
            || options.language_version.supports_language_111())
        && ((kind != "entity" && kind != "relation")
            || options.language_version.supports_relations())
}

#[derive(Debug, Clone)]
pub enum CatalogDecl {
    Alias(crate::navigation::AliasInfo),
    Anchor(WorldDecl),
    AnchorLink(crate::anchors::AnchorLink),
    State(crate::states::StateDecl),
    Tag(WorldDecl),
    Asset(AssetDecl),
    Mark(CatalogLink),
    Attach(CatalogLink),
}

#[derive(Debug, Clone)]
pub struct AssetDecl {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub display: String,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogLink {
    pub target: TargetRef,
    pub values: Vec<String>,
    pub file: String,
    pub line: u32,
    pub inline: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogObject {
    pub target: TargetRef,
    pub display: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceInfo {
    pub source: TargetRef,
    pub target: TargetRef,
    pub kind: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagInfo {
    pub id: String,
    pub display: String,
    pub description: String,
    pub properties: BTreeMap<String, PropertyValue>,
    pub file: String,
    pub line: u32,
    pub declared: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityInfo {
    pub id: String,
    pub entity_type: String,
    pub display: String,
    pub description: String,
    pub properties: BTreeMap<String, PropertyValue>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssetInfo {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub resolved_path: String,
    pub display: String,
    pub file: String,
    pub line: u32,
    pub available: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Catalog {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dynamic_state_changes: Vec<crate::states::StateChangeSite>,
    pub aliases: Vec<crate::navigation::AliasInfo>,
    pub text_links: Vec<crate::navigation::TextLinkInfo>,
    pub anchors: BTreeMap<String, crate::anchors::AnchorInfo>,
    pub states: BTreeMap<String, crate::states::StateInfo>,
    pub objects: Vec<CatalogObject>,
    pub tags: BTreeMap<String, TagInfo>,
    pub entities: BTreeMap<String, EntityInfo>,
    pub assets: BTreeMap<String, AssetInfo>,
    pub marks: Vec<CatalogLink>,
    pub attachments: Vec<CatalogLink>,
    pub references: Vec<ReferenceInfo>,
    pub relation_types: BTreeMap<String, crate::relations::RelationTypeInfo>,
    pub relations: BTreeMap<String, crate::relations::SemanticRelationInfo>,
    pub legacy_relations: Vec<crate::relations::LegacyRelationInfo>,
    /// 对象到关系 ID 的稳定邻接索引；查询只从这里展开局部边。
    #[serde(serialize_with = "serialize_relation_index")]
    pub relation_index: BTreeMap<TargetRef, Vec<String>>,
}

fn serialize_relation_index<S: serde::Serializer>(
    index: &BTreeMap<TargetRef, Vec<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Entry<'a> {
        target: &'a TargetRef,
        relations: &'a [String],
    }
    serializer.collect_seq(
        index
            .iter()
            .map(|(target, relations)| Entry { target, relations }),
    )
}

impl Catalog {
    pub fn object(&self, target: &TargetRef) -> Option<&CatalogObject> {
        self.objects.iter().find(|o| &o.target == target)
    }

    pub fn references_to(&self, target: &TargetRef) -> Vec<ReferenceInfo> {
        self.references
            .iter()
            .filter(|r| &r.target == target)
            .cloned()
            .collect()
    }

    /// 解引用标签。循环可存在,每个标签与目标只访问一次。
    pub fn query(&self, tag: &str, recursive: bool) -> Vec<CatalogObject> {
        let mut queue = VecDeque::from([tag.to_string()]);
        let mut seen = BTreeSet::new();
        let mut targets = BTreeSet::new();
        while let Some(tag) = queue.pop_front() {
            if !seen.insert(tag.clone()) {
                continue;
            }
            for mark in self.marks.iter().filter(|m| m.values.contains(&tag)) {
                if recursive && mark.target.kind == "tag" {
                    queue.push_back(mark.target.id.clone());
                }
                targets.insert(mark.target.clone());
            }
        }
        self.objects
            .iter()
            .filter(|o| targets.contains(&o.target))
            .cloned()
            .collect()
    }

    pub fn tags_for(&self, target: &TargetRef) -> Vec<String> {
        self.marks
            .iter()
            .filter(|m| &m.target == target)
            .flat_map(|m| m.values.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn assets_for(&self, target: &TargetRef) -> Vec<AssetInfo> {
        let ids: BTreeSet<_> = self
            .attachments
            .iter()
            .filter(|a| &a.target == target)
            .flat_map(|a| &a.values)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.assets.get(id).cloned())
            .collect()
    }

    pub(crate) fn add_object(
        &mut self,
        kind: &str,
        id: &str,
        display: &str,
        file: &str,
        line: u32,
    ) {
        let target = TargetRef::new(kind, id);
        if self.object(&target).is_none() {
            self.objects.push(CatalogObject {
                target,
                display: display.into(),
                file: file.into(),
                line,
            });
        }
    }
}

pub fn resolved_asset(file: &str, path: &str) -> std::path::PathBuf {
    crate::compiler::source_path(
        &Path::new(file)
            .parent()
            .unwrap_or(Path::new("."))
            .join(path),
    )
}

pub fn supported_extension(kind: &str, path: &Path) -> bool {
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    match kind {
        "image" => ["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&ext.as_str()),
        "audio" => ["wav", "mp3", "ogg", "flac", "m4a", "aac"].contains(&ext.as_str()),
        "file" => true,
        _ => false,
    }
}
