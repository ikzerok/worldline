use super::*;

#[test]
fn relation_graph_keeps_complete_public_labels_and_clips_scan_summaries() {
    let label = "潮生的登记住所位于镜港，但并不能证明他当夜在家。👩‍👩‍👧‍👦 e\u{301} <&>";
    let source = format!("entity a kind place as \"这是非常漫长的中文起点名称\"\nentity b kind place as \"VeryLongDestinationNameWithoutSpacesAndMore\"\nentity secret kind place as \"CANARY_PRIVATE_ENDPOINT\"\nrelation_type connects as \"相连\"\nrelation_def edge type connects from entity a to entity b\n  description {}\nrelation_def hidden type connects from entity a to entity secret\n  description \"CANARY_PRIVATE_RELATION\"\n",serde_json::to_string(label).unwrap());
    let fixture = Fixture::new("long-graph", &source, "1.10");
    let mut selected = selection();
    selected.objects = ["a", "b"]
        .into_iter()
        .map(|id| TargetRef::new("entity", id))
        .chain(std::iter::once(TargetRef::new("relation", "edge")))
        .collect();
    selected.fields.clear();
    selected.attachments.clear();
    let (preview, files) = package(&fixture.project(), &selected);
    let html = String::from_utf8_lossy(&files[Path::new("relations.html")]);
    assert!(html.contains("viewBox=\"0 0 800 104\""));
    assert_eq!(html.matches("<clipPath ").count(), 3);
    assert!(html.contains("clip-path=\"url(#relation-label-0-1)\""));
    assert!(html.contains("<dl class=\"relation-labels\">"));
    assert!(html.contains("当夜在家。👩‍👩‍👧‍👦 e\u{301} &lt;&amp;&gt;"));
    assert!(html.contains('…'));
    assert!(!all_text(&files).contains("CANARY"));
    let object = &files[Path::new(&route(&preview, "entity", "a"))];
    assert!(String::from_utf8_lossy(object).contains("relation-labels"));
    let item = preview
        .content
        .iter()
        .find(|p| p.output_path == "relations.html")
        .unwrap();
    assert!(item.text.contains(label));
    assert!(item
        .text
        .contains("VeryLongDestinationNameWithoutSpacesAndMore"));
    let css = String::from_utf8_lossy(&files[Path::new("style.css")]);
    assert!(css.contains("overflow-x:auto"));
    assert!(css.contains(".relation-labels{grid-template-columns:1fr}"));
    assert!(!css.contains("max-height:40rem"));
}

#[test]
fn each_public_relation_has_independently_sized_svg_and_unique_clip_anchors() {
    let mut source = String::from("entity a kind place as \"出发地\"\nentity b kind place as \"目标\"\nrelation_type connects as \"连接\"\n");
    let mut selected = selection();
    selected.objects = vec![TargetRef::new("entity", "a"), TargetRef::new("entity", "b")];
    selected.fields.clear();
    selected.attachments.clear();
    for i in 0..30 {
        source.push_str(&format!("relation_def edge_{i} type connects from entity a to entity b\n  description \"公开关系{i}\"\n"));
        selected
            .objects
            .push(TargetRef::new("relation", &format!("edge_{i}")));
    }
    let fixture = Fixture::new("graph-rows", &source, "1.10");
    let (_, files) = package(&fixture.project(), &selected);
    let html = String::from_utf8_lossy(&files[Path::new("relations.html")]);
    assert_eq!(html.matches("viewBox=\"0 0 800 104\"").count(), 30);
    for row in 0..30 {
        for col in 0..3 {
            assert_eq!(
                html.matches(&format!("id=\"relation-label-{row}-{col}\""))
                    .count(),
                1
            );
        }
    }
    selected.objects.retain(|target| target.kind != "relation");
    let (_, files) = package(&fixture.project(), &selected);
    assert!(
        !String::from_utf8_lossy(&files[Path::new("relations.html")])
            .contains("relation-graph-scroll")
    );
}
