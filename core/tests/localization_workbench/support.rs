#![allow(dead_code)]
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::localization::*;
use worldline_core::project::Project;

pub const SOURCE: &str = concat!(
    "let traveler = \"Ari\"\n",
    "event start\n",
    "  Hello {traveler} at [[event:target|Harbor]] 🙂 #wl-localization:greeting\n",
    "  choice \"Continue {traveler}\" #wl-localization:choice\n",
    "    -> END\n",
    "  -> END\n",
    "event target\n",
    "  -> END\n",
);

pub struct Fixture {
    pub root: PathBuf,
    pub project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub fn fixture(name: &str, source: &str, sidecar: Option<serde_json::Value>) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-localization-workbench-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    let mut manifest = serde_json::json!({
        "schema_version":1,"language_version":"1.13","entry":"world.wl",
        "required_features":["content.localization.v1"],"x_manifest":{"sentinel":"keep"}
    });
    if let Some(sidecar) = sidecar {
        manifest["localizations"] =
            serde_json::json!({"zh-Hant":".world/localization/zh-Hant.json"});
        fs::create_dir_all(root.join(".world/localization")).unwrap();
        fs::write(
            root.join(".world/localization/zh-Hant.json"),
            serde_json::to_vec(&sidecar).unwrap(),
        )
        .unwrap();
    }
    fs::write(
        root.join(".world/project.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    Fixture {
        root: project.root.clone(),
        project,
    }
}

pub fn sidecar(entries: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"zh-Hant","entries":entries,"x_sidecar":{"keep":true}})
}

pub fn query() -> LocalizationCatalogQuery {
    LocalizationCatalogQuery {
        target_locale: Some("zh-Hant".into()),
        ..Default::default()
    }
}

pub fn page(project: &Project) -> LocalizationCatalogPage {
    project.query_localization_catalog(&query()).unwrap()
}

pub fn edit(project: &Project, ids: &[&str]) -> LocalizationEditDraft {
    let page = page(project);
    LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hant".into(),
        source_baseline: page.source_baseline,
        edits: ids
            .iter()
            .map(|id| {
                let entry = page
                    .entries
                    .iter()
                    .find(|entry| entry.id.as_deref() == Some(id))
                    .unwrap();
                LocalizationEdit {
                    id: (*id).into(),
                    source_revision: entry.source_revision.clone().unwrap(),
                    translation_parts: translate(&entry.source_parts),
                }
            })
            .collect(),
    }
}

pub fn translate(parts: &[LocalizationPart]) -> Vec<LocalizationPart> {
    parts
        .iter()
        .map(|part| match part {
            LocalizationPart::Text { text } => LocalizationPart::Text {
                text: format!("中文🙂\nمرحبا {text}"),
            },
            LocalizationPart::Placeholder { token } => LocalizationPart::Placeholder {
                token: token.clone(),
            },
            LocalizationPart::Link { token, .. } => LocalizationPart::Link {
                token: token.clone(),
                label: "港口🌊".into(),
            },
        })
        .collect()
}

pub fn apply(project: &mut Project, draft: &LocalizationEditDraft) -> LocalizationImportResult {
    let preview = project.preview_localization_edit(draft).unwrap();
    assert!(preview.can_apply, "{:?}", preview.diagnostics);
    project
        .apply_localization_edit(draft, &preview.plan_digest)
        .unwrap()
}

pub fn disk(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

pub fn id_draft(project: &Project, line: u32, id: &str) -> LocalizationIdDraft {
    let page = page(project);
    let entry = page
        .entries
        .iter()
        .find(|entry| {
            entry
                .source
                .as_ref()
                .is_some_and(|source| source.line == line)
        })
        .unwrap();
    LocalizationIdDraft {
        schema_version: 1,
        source_baseline: page.source_baseline,
        assignments: vec![LocalizationIdAssignment {
            source: entry.source.clone().unwrap(),
            source_revision: entry.source_revision.clone().unwrap(),
            expected_id: entry.id.clone(),
            id: id.into(),
        }],
    }
}

pub fn presentation_request(
    policy: LocalizationPresentationPolicy,
) -> LocalizationPresentationRequest {
    LocalizationPresentationRequest {
        schema_version: 1,
        target_locale: "zh-Hant".into(),
        policy,
    }
}
