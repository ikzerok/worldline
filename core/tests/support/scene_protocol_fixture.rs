use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::map_creation::{create_map, CreateMapRequest};
use worldline_core::presentation_commands::{document_hash, Revision};
use worldline_core::project::Project;
use worldline_core::vector_scene::{self, SceneBatch, SceneGeometry, SceneNode, SceneOp};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
    pub map: PathBuf,
}

impl Fixture {
    pub fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "scene-protocol-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut project = Project::new(&root);
        create_map(
            &mut project,
            &mut Revision::default(),
            CreateMapRequest::new("atlas", "雾港地图", 400, 200),
            BTreeMap::new(),
        )
        .unwrap();
        project.save().unwrap();
        Self {
            map: root.join(".world/maps/atlas.json"),
            root,
        }
    }
    pub fn project(&self) -> Project {
        Project::open(&self.root).unwrap()
    }
    pub fn bytes(&self) -> Vec<u8> {
        std::fs::read(&self.map).unwrap()
    }
    pub fn batch(&self) -> SceneBatch {
        SceneBatch {
            map_id: "atlas".into(),
            expected_revision: Revision::default(),
            expected_documents: BTreeMap::from([(self.map.clone(), document_hash(&self.bytes()))]),
            operations: vec![
                SceneOp::EnableScene,
                SceneOp::Insert {
                    node: SceneNode::new(
                        "rect_1",
                        "places",
                        SceneGeometry::Rect {
                            x: 10.,
                            y: 20.,
                            width: 60.,
                            height: 40.,
                            rx: 0.,
                            ry: 0.,
                        },
                    ),
                    index: None,
                },
            ],
        }
    }
    pub fn preview(&self, batch: &SceneBatch) -> (String, String, serde_json::Value) {
        let project = self.project();
        let baseline = project.content_baseline();
        let plan =
            vector_scene::preview_batch(&project, Revision::default(), batch.clone()).unwrap();
        let digest = worldline_core::scene_protocol::plan_digest(&baseline, batch, &plan).unwrap();
        (baseline, digest, serde_json::to_value(plan).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
