use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    localization::{
        LocalizationPart, LocalizationPresentationPolicy, LocalizationPresentationRequest,
        LocalizationPresentationSnapshot, LocalizationSelection,
    },
    project::Project,
};

pub struct Fixture {
    pub root: PathBuf,
    pub project: Project,
}

pub fn fixture(name: &str, source: &str, ids: &[&str]) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    // 可设置本轮证据目录；不删除失败夹具，便于复查原稿和 sidecar。
    let base = std::env::var_os("WORLDLINE_TEST_ARTIFACT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("worldline-runtime-localization"));
    let root = base.join(format!(
        "{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world/localization")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        serde_json::to_vec(&json!({
            "schema_version":1, "language_version":"1.13", "entry":"world.wl",
            "required_features":["content.localization.v1"],
            "localizations":{"zh-Hant":".world/localization/zh-Hant.json"}
        }))
        .unwrap(),
    )
    .unwrap();
    let sidecar = json!({"schema_version":1,"required_features":["content.localization.v1"],
        "source_locale":"en", "target_locale":"zh-Hant", "entries":{}});
    fs::write(
        root.join(".world/localization/zh-Hant.json"),
        serde_json::to_vec(&sidecar).unwrap(),
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    let mut fixture = Fixture {
        root: project.root.clone(),
        project,
    };
    let compiled = fixture.project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    if !ids.is_empty() {
        fixture.populate(ids);
    }
    fixture
}

impl Fixture {
    pub fn path(&self, relative: &str) -> PathBuf {
        worldline_core::file_access::within(&self.root, &self.root.join(relative)).unwrap()
    }

    pub fn populate(&mut self, ids: &[&str]) {
        let selection = LocalizationSelection {
            schema_version: 1,
            source_locale: "en".into(),
            target_locale: "zh-Hant".into(),
            string_ids: ids.iter().map(|id| (*id).into()).collect(),
        };
        let export = self
            .project
            .preview_localization_export(&selection)
            .unwrap();
        assert!(export.can_export, "{:?}", export.diagnostics);
        let mut document = self.sidecar();
        for entry in export.exchange.entries {
            document["entries"][&entry.id] = json!({"source_revision":entry.source_revision,
                "translation_parts":entry.source_parts});
        }
        self.write_sidecar(document);
    }
    pub fn sidecar(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.root.join(".world/localization/zh-Hant.json")).unwrap(),
        )
        .unwrap()
    }
    pub fn write_sidecar(&mut self, sidecar: Value) {
        fs::write(
            self.root.join(".world/localization/zh-Hant.json"),
            serde_json::to_vec(&sidecar).unwrap(),
        )
        .unwrap();
        self.project = Project::open(&self.root).unwrap();
    }
    pub fn translate(&mut self, id: &str, parts: Vec<LocalizationPart>) {
        let mut sidecar = self.sidecar();
        sidecar["entries"][id]["translation_parts"] = serde_json::to_value(parts).unwrap();
        self.write_sidecar(sidecar);
    }
    pub fn presentation(
        &self,
        policy: LocalizationPresentationPolicy,
    ) -> LocalizationPresentationSnapshot {
        self.project
            .prepare_localization_presentation(&request(policy))
            .unwrap()
    }
}

pub fn request(policy: LocalizationPresentationPolicy) -> LocalizationPresentationRequest {
    LocalizationPresentationRequest {
        schema_version: 1,
        target_locale: "zh-Hant".into(),
        policy,
    }
}
pub fn text(value: &str) -> LocalizationPart {
    LocalizationPart::Text { text: value.into() }
}
pub fn placeholder(value: &str) -> LocalizationPart {
    LocalizationPart::Placeholder {
        token: value.into(),
    }
}
pub fn link(token: &str, label: &str) -> LocalizationPart {
    LocalizationPart::Link {
        token: token.into(),
        label: label.into(),
    }
}
