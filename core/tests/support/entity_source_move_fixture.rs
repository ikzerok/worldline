#![allow(dead_code)]
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::{SourceLifecyclePlan, SourceLifecycleRequest};

pub const SOURCE: &str = "设定/原稿.wl";
pub const TARGET: &str = "资料/地点 空间.wl";
pub const ID: &str = "north_lighthouse";
pub const DECLARATION: &str = concat!(
    "entity north_lighthouse kind place as \"北方灯塔🙂\" // 同行注释\r\n",
    "  description \"雾港\\\"旧灯\\\"，Unicode café\"\r\n",
    "  /* 块内注释\r\n     必须原样保留 */\r\n",
    "  property height = 38\r\n",
    "  property lit = true\r\n",
    "  property note = \"north_lighthouse 与 资料/地点 空间.wl\"\r\n",
    "  property owner = ref(\"entity\", \"keepers\")\r\n",
    "  property route = ref(\"relation\", \"edge\")\r\n",
    "  property guide = ref(\"character\", \"guide\") // 属性同行\r\n",
);
pub const PREFIX: &str = "// 文件前言🙂\r\n\r\n// 独立说明，不归实体\r\n";
pub const SUFFIX: &str = "\r\n\r\n// 相邻实体的说明\r\nentity neighboring kind place as \"邻居\"\r\n  description \"邻居资料\"\r\n  property watched = ref(\"entity\", \"north_lighthouse\")\r\n";
pub const TARGET_TEXT: &str =
    "// 目标前言\r\nentity keepers kind organization as \"守灯会\"\r\n// 末尾无换行的独立注释🙂";
pub const WORLD: &str = concat!(
    "include \"设定/原稿.wl\"\ninclude \"资料/地点 空间.wl\"\n",
    "character guide as \"向导\"\n",
    "relation_type guards as \"守护\"\n",
    "relation_def edge type guards from entity keepers to entity north_lighthouse\n",
    "  scope_ref entity north_lighthouse\n",
    "alias entity north_lighthouse as \"灯塔别名\"\n",
    "tag places as \"地点\"\nmark entity north_lighthouse with places\n",
    "asset picture file \"assets/picture.txt\" as \"原图\"\n",
    "attach entity north_lighthouse with picture\n",
    "anchor_def harbor as \"港湾\"\nanchor_link harbor entity north_lighthouse\n",
    "state lamp on entity north_lighthouse with places as \"灯光\"\n",
    "schema lighthouse_record for entity entity_type place\n",
    "  field height_field height number required\n",
    "  field owner_field owner ref entity entity_type organization required\n",
    "bind entity north_lighthouse to lighthouse_record\n",
    "event start with guide\n",
    "  [[entity:north_lighthouse|灯塔]] [[entity:north_lighthouse|灯塔]]\n",
    "  choice \"望向 [[entity:north_lighthouse|灯塔]]\"\n    -> END\n",
);

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new(source: &str, target: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-entity-move-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Self {
            root: Project::new(&root).root,
        };
        fixture.write(SOURCE, source);
        fixture.write(TARGET, target);
        fixture.write(
            "world.wl",
            &format!("include \"{SOURCE}\"\ninclude \"{TARGET}\"\nevent start\n  原稿\n  -> END\n"),
        );
        fixture.write("archive.wl", "entity secret kind place\n");
        fixture.write("inactive.wl", "entity inactive kind place\n");
        fixture.write("assets/picture.txt", "附件原始字节🙂\n");
        fixture.write(".hidden/unused.bin", "未引用隐藏文件\n");
        fixture.write(
            ".world/project.json",
            &json!({
                "schema_version":1,"language_version":"1.13","entry":"world.wl",
                "required_features":["workspace.source_sets.v1","content.entities.v1",
                    "content.relations.v1","content.object_refs.v1","content.character_refs.v1"],
                "source_config":{"mode":"explicit","active":["world.wl",SOURCE,TARGET],
                    "archived":["archive.wl"]},
                "extension":{"keep":"north_lighthouse"}
            })
            .to_string(),
        );
        fixture
    }

    pub fn simple() -> Self {
        Self::new(
            "entity north_lighthouse kind place as \"北方灯塔\"\n",
            "// 活动目标\n",
        )
    }

    pub fn full() -> Self {
        let fixture = Self::new(&format!("{PREFIX}{DECLARATION}{SUFFIX}"), TARGET_TEXT);
        fixture.write("world.wl", WORLD);
        fixture
    }

    pub fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub fn project(&self) -> Project {
        Project::open(&self.root).unwrap()
    }

    pub fn request(&self, to: &str) -> SourceLifecycleRequest {
        SourceLifecycleRequest::MoveEntity {
            id: ID.into(),
            to: to.into(),
        }
    }

    pub fn plan(&self) -> SourceLifecyclePlan {
        self.project()
            .preview_source_lifecycle(&self.request(TARGET))
            .unwrap()
    }

    pub fn bytes(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut result = BTreeMap::new();
        collect(&self.root, &self.root, &mut result);
        result
    }
}

fn collect(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(root).unwrap();
        if relative == Path::new(".world/.transactions") {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, result);
        } else {
            result.insert(relative.to_path_buf(), std::fs::read(&path).unwrap());
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
