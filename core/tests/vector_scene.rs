//! 新矢量模型的原子编辑、安全交换与兼容回归。
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use worldline_core::{
    map_creation::{self, CreateMapRequest, MISSING_DOCUMENT_HASH},
    presentation_commands::{self, document_hash, Revision},
    project::Project,
    svg_import,
    vector_scene::*,
};
#[path = "vector_scene/dashes.rs"]
mod dashes;
#[path = "vector_scene/migration.rs"]
mod migration;
#[path = "vector_scene/performance.rs"]
mod performance;
#[path = "vector_scene/security.rs"]
mod security;
#[path = "vector_scene/structure.rs"]
mod structure;
#[path = "vector_scene/transactions.rs"]
mod transactions;

fn project(name: &str) -> (Project, Revision) {
    let root = std::env::temp_dir().join(format!(
        "scene-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut p = Project::new(&root);
    let mut r = Revision::default();
    let baseline = BTreeMap::from([(
        p.root.join(".world/project.json"),
        MISSING_DOCUMENT_HASH.into(),
    )]);
    map_creation::create_map(
        &mut p,
        &mut r,
        CreateMapRequest::new("map", "地图", 200, 100),
        baseline,
    )
    .unwrap();
    (p, r)
}
fn path(p: &Project) -> PathBuf {
    p.root.join(".world/maps/map.json")
}
fn baseline(p: &Project) -> BTreeMap<PathBuf, String> {
    let path = path(p);
    BTreeMap::from([(
        path.clone(),
        document_hash(p.authoring_document(&path).unwrap().bytes()),
    )])
}
fn batch(p: &Project, r: Revision, operations: Vec<SceneOp>) -> SceneBatch {
    SceneBatch {
        map_id: "map".into(),
        expected_revision: r,
        expected_documents: baseline(p),
        operations,
    }
}
fn apply(
    p: &mut Project,
    r: &mut Revision,
    operations: Vec<SceneOp>,
) -> presentation_commands::CommandResult {
    let plan = preview_batch(p, *r, batch(p, *r, operations)).unwrap();
    apply_batch(p, r, &plan).unwrap()
}
fn point(id: &str, x: f64) -> SceneNode {
    SceneNode::new(
        id,
        "places",
        SceneGeometry::Point {
            position: [x, 20.0],
        },
    )
}
fn map(p: &Project) -> worldline_core::MapDocument {
    p.map_index().maps.remove("map").unwrap()
}
fn scene(p: &Project) -> MapScene {
    map(p).scene.unwrap()
}
fn source() -> String {
    "<svg width='300' height='100' viewBox='0 0 100 100' preserveAspectRatio='xMidYMid slice'><g fill='red'><path d='M0 0C10 10 20 30 40 40A5 6 20 01 70 80Z'/><text x='2' y='20'>灯塔</text></g></svg>".into()
}

fn compile_snapshot(project: &Project) -> worldline_core::CompileResult {
    project.clone().compile()
}
