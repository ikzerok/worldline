use crate::{
    catalog::{Catalog, ReferenceInfo, TargetRef},
    project::Project,
    CompileResult,
};
use serde_json::Value;
use std::path::Path;

pub(super) fn equivalent(
    before_project: &Project,
    after_project: &Project,
    before: &CompileResult,
    old: &Path,
    new: &Path,
) -> Result<(), String> {
    let after = after_project.compile_current();
    if after.has_errors() {
        let error = after
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
            .unwrap();
        return Err(format!("移动候选无效：{} {}", error.code, error.message));
    }
    let old = old.to_string_lossy();
    let new = new.to_string_lossy();
    let map = |value: &str| {
        if value == old {
            new.to_string()
        } else {
            value.to_owned()
        }
    };
    let files: Vec<_> = before.program.files.iter().map(|file| map(file)).collect();
    if files != after.program.files || before.program.entry != after.program.entry {
        return Err(
            "路径移动会改变语义加载顺序或默认入口；请先显式安排 include 顺序再重新预览，工程未修改"
                .into(),
        );
    }
    if before.analysis.fingerprint != after.analysis.fingerprint {
        return Err(format!("移动改变运行身份/指纹（{} → {}）；旧 Story 存档和检查点不可直接沿用，trace 须重新验证。本版不迁移，工程未修改", before.analysis.fingerprint, after.analysis.fingerprint));
    }
    // 有类型的映射只改变正式路径字段；作者资料和值始终完整参加比较。
    let left = normalized_catalog(&before.analysis.catalog, &old, &new)?;
    let right = normalized_catalog(&after.analysis.catalog, &new, &new)?;
    if left != right {
        return Err(
            "移动候选的对象身份、资料或正式引用目标不同，无法证明语义等价，工程未修改".into(),
        );
    }
    if before_project.compile_options() != after_project.compile_options() {
        return Err("移动不能改变语言版本或 required feature".into());
    }
    Ok(())
}

fn normalized_catalog(catalog: &Catalog, old: &str, new: &str) -> Result<Value, String> {
    let mut catalog = catalog.clone();
    let location = |file: &mut String| {
        if file == old {
            *file = new.into();
        }
    };
    let target = |target: &mut TargetRef| {
        if target.kind == "file" {
            location(&mut target.id);
        }
    };
    let change = |change: &mut crate::states::StateChangeSite| {
        location(&mut change.file);
        if let Some(source) = &mut change.source {
            target(source);
        }
    };
    for object in &mut catalog.objects {
        if object.target.kind == "file" && object.target.id == old {
            // 正式文件对象的缺省显示名随身份映射，不处理任何作者显示名。
            object.display = Path::new(new)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
        }
        target(&mut object.target);
        location(&mut object.file);
    }
    for reference in &mut catalog.references {
        target(&mut reference.source);
        target(&mut reference.target);
        location(&mut reference.file);
    }
    for alias in &mut catalog.aliases {
        target(&mut alias.target);
        location(&mut alias.file);
    }
    for link in &mut catalog.text_links {
        target(&mut link.source);
        target(&mut link.target);
        location(&mut link.file);
        // 精确 token 重基可改变同一行后续链接的列；顺序、行、目标和 label 仍比较。
        link.column = 0;
    }
    for link in catalog.marks.iter_mut().chain(&mut catalog.attachments) {
        target(&mut link.target);
        location(&mut link.file);
    }
    for anchor in catalog.anchors.values_mut() {
        location(&mut anchor.file);
        for link in &mut anchor.links {
            target(&mut link.target);
            location(&mut link.file);
        }
    }
    for state in catalog.states.values_mut() {
        target(&mut state.target);
        location(&mut state.file);
        for site in &mut state.changes {
            change(site);
        }
    }
    for site in &mut catalog.dynamic_state_changes {
        change(site);
    }
    for tag in catalog.tags.values_mut() {
        location(&mut tag.file);
    }
    for entity in catalog.entities.values_mut() {
        location(&mut entity.file);
    }
    for asset in catalog.assets.values_mut() {
        location(&mut asset.file);
        // 资源证明已比较 resolved_path 与原始字节；仅相对书写形式获准重基。
        asset.path.clear();
    }
    for relation_type in catalog.relation_types.values_mut() {
        location(&mut relation_type.file);
    }
    for relation in catalog.relations.values_mut() {
        location(&mut relation.file);
        target(&mut relation.from_ref);
        target(&mut relation.to_ref);
        for scope in &mut relation.scope_refs {
            target(scope);
        }
    }
    for relation in &mut catalog.legacy_relations {
        target(&mut relation.handle.source);
        target(&mut relation.handle.target);
        location(&mut relation.handle.file);
    }
    // 只重排正式派生索引。所有 Vec 保留重复项，有序来源/动作列表不被集合化。
    catalog.objects.sort_by(|a, b| a.target.cmp(&b.target));
    catalog.references.sort_by(ReferenceInfo::canonical_cmp);
    catalog.relation_index = catalog
        .relation_index
        .into_iter()
        .map(|(mut key, relations)| {
            target(&mut key);
            (key, relations)
        })
        .collect();
    serde_json::to_value(catalog).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
