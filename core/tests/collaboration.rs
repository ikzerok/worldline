use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::collaboration::{
    self, AnchorStatus, ApplyProposalCommand, CommentAnchor, CommentCommand, CommentDraft,
    ProposalCommand, ProposalDraft, ProposalFileChange, ProposalResolution, ProposalStatus,
};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;
use worldline_core::TargetRef;

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-collab-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn map_json(p1_x: i64, p2_y: i64, layer_order: &[&str], include_p1: bool) -> String {
    let mut placements = serde_json::Map::new();
    if include_p1 {
        placements.insert(
            "p1".into(),
            serde_json::json!({
                "layer_id":"a","target_ref":{"kind":"entity","id":"a"},
                "geometry":{"kind":"point","position":[0.1,0.1]},
                "annotation":"","role":"资料入口","scope_refs":[],"x":p1_x
            }),
        );
    }
    placements.insert(
        "p2".into(),
        serde_json::json!({
            "layer_id":"b","target_ref":{"kind":"entity","id":"b"},
            "geometry":{"kind":"point","position":[0.2,0.2]},
            "annotation":"","role":"资料入口","scope_refs":[],"y":p2_y
        }),
    );
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version":1,"id":"city","title":"城市","raster_layers":[],
        "canvas":{"width":1000,"height":800,"unit":"normalized"},
        "layer_order":layer_order,
        "layers":{
            "a":{"title":"A","visible_default":true,"locked":false},
            "b":{"title":"B","visible_default":true,"locked":false}
        },
        "placements":placements,"extensions":{}
    }))
    .unwrap()
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "entity a kind place as \"甲\"\nentity b kind place as \"乙\"\nrelation_type knows as \"认识\"\nrelation_def rel type knows from entity a to entity b\n",
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{
          "schema_version":1,"language_version":"1.10","entry":"world.wl",
          "required_features":["content.entities.v1","content.relations.v1","presentation.maps.v1"],
          "maps":{"city":".world/maps/city.json"},"graph_views":{},"presets":{}
        }"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/city.json"),
        map_json(0, 0, &["a", "b"], true),
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn proposal(id: &str, base: String, proposed: String) -> ProposalDraft {
    ProposalDraft {
        id: id.into(),
        author: "作者甲".into(),
        reason: "需要明确审阅的改动".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: ".world/maps/city.json".into(),
            domain: "presentation".into(),
            base: Some(base),
            proposed: Some(proposed),
        }],
    }
}

fn content_proposal(id: &str, path: &str, base: &str, proposed: &str) -> ProposalDraft {
    ProposalDraft {
        id: id.into(),
        author: "作者甲".into(),
        reason: "需要明确审阅的改动".into(),
        status: ProposalStatus::Open,
        changes: vec![ProposalFileChange {
            path: path.into(),
            domain: "content".into(),
            base: Some(base.into()),
            proposed: Some(proposed.into()),
        }],
    }
}

#[path = "collaboration/comments.rs"]
mod comments;
#[path = "collaboration/proposal_capture.rs"]
mod proposal_capture;
#[path = "collaboration/proposal_resolution.rs"]
mod proposal_resolution;
#[path = "collaboration/proposal_review_differences.rs"]
mod proposal_review_differences;
#[path = "collaboration/proposal_review_impacts.rs"]
mod proposal_review_impacts;
