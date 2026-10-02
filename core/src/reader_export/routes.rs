use super::*;

pub(super) fn hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub(super) fn target_route(
    selection: &ReaderExportSelection,
    target: &TargetRef,
    legacy: String,
    overrides: &[ReaderProfileRoute],
) -> Result<String, String> {
    let route = if let Some(saved) = overrides
        .iter()
        .find(|route| route.target.as_ref() == Some(target))
    {
        saved.output_path.clone()
    } else if selection.schema_version == READER_SITE_SCHEMA_VERSION {
        let hash = hash_bytes(
            &serde_json::to_vec(&[&target.kind, &target.id]).map_err(|e| e.to_string())?,
        );
        let folder = match target.kind.as_str() {
            "asset" => "assets",
            "map" => "maps",
            _ => "objects",
        };
        let extension = legacy
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .ok_or("公开路径缺少扩展名")?;
        format!("{folder}/r{hash}.{extension}")
    } else if !overrides.is_empty() {
        new_target_ordinal(selection, target, &legacy, overrides)?
    } else {
        legacy.clone()
    };
    if target.kind == "asset"
        && route.rsplit_once('.').map(|(_, ext)| ext) != legacy.rsplit_once('.').map(|(_, ext)| ext)
    {
        return Err("发布配置不能改变附件扩展名，请重新核对路由".into());
    }
    Ok(route)
}

pub(super) fn chapter_route(
    selection: &ReaderExportSelection,
    manuscript_id: &str,
    chapter_id: &str,
    legacy: String,
    overrides: &[ReaderProfileRoute],
) -> Result<String, String> {
    if let Some(route) = overrides.iter().find(|route| {
        route.target.is_none()
            && route.manuscript_id.as_deref() == Some(manuscript_id)
            && route.chapter_id.as_deref() == Some(chapter_id)
    }) {
        return Ok(route.output_path.clone());
    }
    if selection.schema_version != READER_SITE_SCHEMA_VERSION {
        if overrides.is_empty() {
            return Ok(legacy);
        }
        let reserved: BTreeSet<_> = overrides
            .iter()
            .filter_map(|route| {
                route
                    .manuscript_id
                    .as_deref()
                    .zip(route.chapter_id.as_deref())
            })
            .collect();
        let book = selection
            .manuscripts
            .iter()
            .find(|book| book.id == manuscript_id)
            .ok_or("公开书稿不存在")?;
        let rank = book
            .chapters
            .iter()
            .filter(|id| !reserved.contains(&(manuscript_id, id.as_str())))
            .position(|id| id == chapter_id)
            .ok_or("新增章节路由身份无效")?;
        let (prefix, _) = legacy.rsplit_once("-c").ok_or("旧章节路由无效")?;
        return free_ordinal(&format!("{prefix}-c"), "html", rank, overrides);
    }
    let bytes =
        serde_json::to_vec(&["chapter", manuscript_id, chapter_id]).map_err(|e| e.to_string())?;
    Ok(format!("manuscripts/r{}.html", hash_bytes(&bytes)))
}

fn new_target_ordinal(
    selection: &ReaderExportSelection,
    target: &TargetRef,
    legacy: &str,
    overrides: &[ReaderProfileRoute],
) -> Result<String, String> {
    let reserved: BTreeSet<_> = overrides
        .iter()
        .filter_map(|route| route.target.as_ref())
        .collect();
    let (prefix, mut candidates) = match target.kind.as_str() {
        "asset" => (
            "assets/a",
            selection
                .attachments
                .iter()
                .map(|id| TargetRef::new("asset", id))
                .collect::<Vec<_>>(),
        ),
        "map" => (
            "maps/m",
            selection
                .maps
                .iter()
                .map(|map| TargetRef::new("map", &map.id))
                .collect(),
        ),
        _ => {
            let mut objects = selection.objects.clone();
            objects.sort();
            ("objects/o", objects)
        }
    };
    candidates.retain(|candidate| !reserved.contains(candidate));
    let rank = candidates
        .iter()
        .position(|candidate| candidate == target)
        .ok_or("新增公开路由身份无效")?;
    let (_, extension) = legacy.rsplit_once('.').ok_or("公开路径缺少扩展名")?;
    free_ordinal(prefix, extension, rank, overrides)
}

fn free_ordinal(
    prefix: &str,
    extension: &str,
    rank: usize,
    overrides: &[ReaderProfileRoute],
) -> Result<String, String> {
    let reserved: BTreeSet<_> = overrides
        .iter()
        .map(|route| route.output_path.as_str())
        .collect();
    let mut remaining = rank;
    for number in 1..=reserved.len().saturating_add(rank).saturating_add(1) {
        let path = format!("{prefix}{number:04}.{extension}");
        if !reserved.contains(path.as_str()) {
            if remaining == 0 {
                return Ok(path);
            }
            remaining -= 1;
        }
    }
    Err("无法分配不冲突的公开路由".into())
}

pub(super) fn route_from_included(entry: &ReaderExportIncluded) -> ReaderProfileRoute {
    ReaderProfileRoute {
        target: entry.target.clone(),
        manuscript_id: entry.manuscript_id.clone(),
        chapter_id: entry.chapter_id.clone(),
        output_path: entry.output_path.clone(),
    }
}

pub(super) fn validate_profile_routes(routes: &[ReaderProfileRoute]) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for route in routes {
        let (folder, legacy_prefix) = match (&route.target, &route.manuscript_id, &route.chapter_id)
        {
            (Some(target), None, None) if target.kind == "asset" => ("assets", 'a'),
            (Some(target), None, None) if target.kind == "map" => ("maps", 'm'),
            (Some(target), None, None) if super::plan::public_object_kind(&target.kind) => {
                ("objects", 'o')
            }
            (None, Some(book), Some(chapter)) if !book.is_empty() && !chapter.is_empty() => {
                ("manuscripts", 'm')
            }
            _ => return Err("发布配置路由身份无效".into()),
        };
        let Some((actual_folder, filename)) = route.output_path.split_once('/') else {
            return Err("发布配置路由必须使用安全分类目录".into());
        };
        let Some((stem, extension)) = filename.rsplit_once('.') else {
            return Err("发布配置路由缺少扩展名".into());
        };
        let generated = stem.len() == 17
            && stem.starts_with('r')
            && stem[1..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let ordinal = if folder == "manuscripts" {
            stem.split_once("-c")
                .is_some_and(|(book, chapter)| ordinal_stem(book, 'm') && digits(chapter))
        } else {
            ordinal_stem(stem, legacy_prefix)
        };
        let extension_ok = if folder == "assets" {
            super::inputs::allowed_asset_extension(extension)
        } else {
            extension == "html"
        };
        if folder != actual_folder || !(generated || ordinal) || !extension_ok {
            return Err("发布配置路由不是允许的生成路径".into());
        }
        if !paths.insert(&route.output_path)
            || !identities.insert((&route.target, &route.manuscript_id, &route.chapter_id))
        {
            return Err("发布配置包含重复身份或输出路由".into());
        }
    }
    Ok(())
}

fn ordinal_stem(value: &str, prefix: char) -> bool {
    value.strip_prefix(prefix).is_some_and(digits)
}

fn digits(value: &str) -> bool {
    value.len() >= 4 && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn validate_unique_paths(
    pages: &[PublicPage],
    attachments: &[PublicAttachment],
) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for path in pages
        .iter()
        .map(|p| &p.output_path)
        .chain(attachments.iter().map(|a| &a.output_path))
    {
        if !paths.insert(path) {
            return Err("公开内容的稳定路由发生冲突，请调整发布配置".into());
        }
    }
    Ok(())
}

pub(super) fn public_anchor(map: &str, node: &str) -> String {
    // 使用生成的稳定名称，不直接把作者节点 ID 放入公共 URL。
    let bytes = serde_json::to_vec(&[map, node]).expect("字符串数组可以序列化");
    format!("n{}", hash_bytes(&bytes))
}
