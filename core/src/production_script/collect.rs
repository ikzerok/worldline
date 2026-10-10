use super::*;
use crate::ast::{Loc, Stmt};
use crate::localization::SourceIdentity;
use crate::source_provenance::{ExpressionSlot, SourceOwner, StatementKind};
use crate::CompileResult;
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

#[derive(Serialize)]
pub(super) struct Unit {
    pub identity: SourceIdentity,
    pub kind: ProductionKind,
    pub declaration: TargetRef,
    pub root: TargetRef,
    pub source: ProductionSource,
    pub speaker: Option<ProductionSpeaker>,
    pub direction: Option<String>,
    pub controls: Vec<ProductionControl>,
}
pub(super) struct Collected {
    pub definitions: Vec<ProductionDefinition>,
    pub calls: Vec<ProductionCallUse>,
    pub units: Vec<Unit>,
    pub added_fragments: usize,
}
pub(super) fn collect(
    compiled: &CompileResult,
    root: &Path,
    roots: &BTreeSet<TargetRef>,
    request: &ProductionScriptRequest,
    source_ids: &BTreeMap<PathBuf, u32>,
) -> Result<Collected, ProductionError> {
    let mut walker = Walker {
        compiled,
        root,
        request,
        source_ids,
        definitions: BTreeMap::new(),
        calls: BTreeMap::new(),
        new_callees: BTreeSet::new(),
        units: BTreeMap::new(),
        bytes: 0,
        nodes: 0,

        offsets: compiled
            .sources
            .iter()
            .map(|(path, text)| {
                let mut offsets = vec![0];
                offsets.extend(text.match_indices('\n').map(|(at, _)| at + 1));
                (path.clone(), offsets)
            })
            .collect(),
    };
    let mut queue: VecDeque<_> = roots.iter().cloned().collect();
    let mut visited = BTreeSet::new();
    let mut scheduled = roots.clone();
    while let Some(target) = queue.pop_front() {
        if !visited.insert(target.clone()) {
            continue;
        }
        walker.definition(&target)?;
        let discovered = std::mem::take(&mut walker.new_callees);
        if request.include_fragments {
            for callee in discovered {
                if scheduled.insert(callee.clone()) {
                    queue.push_back(callee);
                }
            }
            if queue.len()
                > request
                    .limits
                    .call_sites
                    .saturating_add(request.limits.definitions)
            {
                return Err(ProductionError::budget());
            }
        }
    }
    let added_fragments = walker
        .definitions
        .keys()
        .filter(|target| target.kind == "fragment" && !roots.contains(*target))
        .count();
    let mut definitions: Vec<_> = walker.definitions.into_values().collect();
    for definition in &mut definitions {
        definition.is_root = roots.contains(&definition.target);
    }
    definitions.sort_by(|a, b| {
        (&a.source.file, a.source.line, a.source.column, &a.target).cmp(&(
            &b.source.file,
            b.source.line,
            b.source.column,
            &b.target,
        ))
    });
    // SourceId 只负责身份；公开顺序仍按既有相对路径文本，不使用内部编号排序。
    let mut calls: Vec<_> = walker.calls.into_values().collect();
    calls.sort_by(|a, b| {
        (&a.source.file, a.source.line, a.source.column).cmp(&(
            &b.source.file,
            b.source.line,
            b.source.column,
        ))
    });
    let mut units: Vec<_> = walker.units.into_values().collect();
    units.sort_by(|a, b| {
        (&a.source.file, a.source.line, a.source.column, a.kind).cmp(&(
            &b.source.file,
            b.source.line,
            b.source.column,
            b.kind,
        ))
    });
    Ok(Collected {
        definitions,
        calls,
        units,
        added_fragments,
    })
}
struct Walker<'a> {
    compiled: &'a CompileResult,
    root: &'a Path,
    request: &'a ProductionScriptRequest,
    source_ids: &'a BTreeMap<PathBuf, u32>,
    definitions: BTreeMap<TargetRef, ProductionDefinition>,
    calls: BTreeMap<(u32, u32, u32), ProductionCallUse>,
    new_callees: BTreeSet<TargetRef>,
    units: BTreeMap<(SourceIdentity, u32), Unit>,
    offsets: BTreeMap<PathBuf, Vec<usize>>,
    bytes: usize,
    nodes: usize,
}
impl Walker<'_> {
    fn reserve(&mut self, value: &impl Serialize) -> Result<(), ProductionError> {
        self.bytes = self
            .bytes
            .checked_add(bounded_size(
                value,
                self.request.limits.result_bytes.saturating_sub(self.bytes),
            )?)
            .ok_or_else(ProductionError::budget)?;
        Ok(())
    }
    fn source(&self, file: &str, loc: Loc) -> Result<ProductionSource, ProductionError> {
        let path = Path::new(file);
        if !self.compiled.sources.contains_key(path) {
            return Err(ProductionError::source());
        }
        let relative = path
            .strip_prefix(self.root)
            .map_err(|_| ProductionError::source())?;
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(ProductionError::source());
        }
        Ok(ProductionSource {
            file: relative
                .to_str()
                .ok_or_else(ProductionError::source)?
                .replace('\\', "/"),
            line: loc.line,
            column: loc.column,
        })
    }
    fn add_definition(&mut self, target: &TargetRef) -> Result<(), ProductionError> {
        if self.definitions.contains_key(target) {
            return Ok(());
        }
        if self.definitions.len() >= self.request.limits.definitions {
            return Err(ProductionError::budget());
        }
        let object = self
            .compiled
            .analysis
            .catalog
            .object(target)
            .ok_or_else(ProductionError::source)?;
        let source = if target.kind == "scene" {
            self.scene_source(target)?
        } else {
            self.source(&object.file, Loc::new(object.line, 1))?
        };
        let value = ProductionDefinition {
            target: target.clone(),
            source,
            is_root: false,
        };
        self.reserve(&value)?;
        self.definitions.insert(target.clone(), value);
        Ok(())
    }
    fn scene_source(&self, target: &TargetRef) -> Result<ProductionSource, ProductionError> {
        let path = self
            .compiled
            .analysis
            .symbols
            .scenes
            .get(&target.id)
            .ok_or_else(ProductionError::source)?;
        let event = self
            .compiled
            .program
            .events
            .get(path.event)
            .ok_or_else(ProductionError::source)?;
        let file = self
            .compiled
            .program
            .event_files
            .get(path.event)
            .ok_or_else(ProductionError::source)?;
        let owner = SourceOwner::new(file, event.loc.line);
        let mut body = event.body.as_slice();
        let mut source = None;
        for name in &path.scenes {
            let scene = body
                .iter()
                .find_map(|stmt| match stmt {
                    Stmt::Scene(scene) if &scene.name == name => Some(scene),
                    _ => None,
                })
                .ok_or_else(ProductionError::source)?;
            let physical = self
                .compiled
                .program
                .source_provenance
                .statement_file(&owner, scene.loc, StatementKind::Scene)
                .ok_or_else(ProductionError::source)?;
            source = Some(self.source(physical, scene.loc)?);
            body = &scene.body;
        }
        source.ok_or_else(ProductionError::source)
    }
    fn definition(&mut self, target: &TargetRef) -> Result<(), ProductionError> {
        self.add_definition(target)?;
        let compiled = self.compiled;
        match target.kind.as_str() {
            "event" => {
                let index = compiled
                    .program
                    .event_index(&target.id)
                    .ok_or_else(ProductionError::source)?;
                let event = &compiled.program.events[index];
                let file = compiled
                    .program
                    .event_files
                    .get(index)
                    .ok_or_else(ProductionError::source)?;
                let owner = SourceOwner::new(file, event.loc.line);
                let mut context = Vec::new();
                if event.after.is_some() {
                    let mut control =
                        self.control("event_after", self.source(file, event.loc)?, None);
                    control.condition =
                        Some(self.expression(file, event.loc.line, ExpressionSlot::After)?);
                    context.push(control);
                }
                self.body(&event.body, &owner, target, target, &context, 0)
            }
            "fragment" => {
                let fragment = compiled
                    .program
                    .fragments
                    .iter()
                    .find(|fragment| fragment.name == target.id)
                    .ok_or_else(ProductionError::source)?;
                self.body(
                    &fragment.body,
                    &SourceOwner::new(&fragment.file, fragment.loc.line),
                    target,
                    target,
                    &[],
                    0,
                )
            }
            "scene" => {
                let path = compiled
                    .analysis
                    .symbols
                    .scenes
                    .get(&target.id)
                    .ok_or_else(ProductionError::source)?;
                let event = compiled
                    .program
                    .events
                    .get(path.event)
                    .ok_or_else(ProductionError::source)?;
                let file = compiled
                    .program
                    .event_files
                    .get(path.event)
                    .ok_or_else(ProductionError::source)?;
                let owner = SourceOwner::new(file, event.loc.line);
                let mut body = event.body.as_slice();
                let mut controls = Vec::new();
                if event.after.is_some() {
                    let mut control =
                        self.control("event_after", self.source(file, event.loc)?, None);
                    control.condition =
                        Some(self.expression(file, event.loc.line, ExpressionSlot::After)?);
                    controls.push(control);
                }
                let mut scene_id = event.name.clone();
                for name in &path.scenes {
                    let scene = body
                        .iter()
                        .find_map(|stmt| match stmt {
                            Stmt::Scene(scene) if &scene.name == name => Some(scene),
                            _ => None,
                        })
                        .ok_or_else(ProductionError::source)?;
                    let physical = compiled
                        .program
                        .source_provenance
                        .statement_file(&owner, scene.loc, StatementKind::Scene)
                        .ok_or_else(ProductionError::source)?;
                    scene_id.push('.');
                    scene_id.push_str(name);
                    let scene_target = TargetRef::new("scene", &scene_id);
                    controls.push(self.control(
                        "scene",
                        self.source(physical, scene.loc)?,
                        Some(scene_target),
                    ));
                    body = &scene.body;
                }
                self.body(body, &owner, target, target, &controls, 0)
            }
            "entity" => Ok(()),
            _ => Err(ProductionError::source()),
        }
    }
    fn body(
        &mut self,
        body: &[Stmt],
        owner: &SourceOwner,
        declaration: &TargetRef,
        root: &TargetRef,
        controls: &[ProductionControl],
        depth: usize,
    ) -> Result<(), ProductionError> {
        if depth > 64 {
            return Err(ProductionError::budget());
        }
        for statement in body {
            self.nodes += 1;
            if self.nodes > 200_000 {
                return Err(ProductionError::budget());
            }
            let loc = crate::language::statement_loc(statement);
            let file = self
                .compiled
                .program
                .source_provenance
                .statement_file(owner, loc, StatementKind::of(statement))
                .ok_or_else(ProductionError::source)?;
            let source_id = *self
                .source_ids
                .get(Path::new(file))
                .ok_or_else(ProductionError::source)?;
            let source = self.source(file, loc)?;
            match statement {
                Stmt::Say(say) => {
                    let target = TargetRef::new("character", &say.speaker);
                    if self
                        .request
                        .speaker
                        .as_ref()
                        .is_none_or(|selected| selected == &target)
                    {
                        let display = self
                            .compiled
                            .analysis
                            .catalog
                            .object(&target)
                            .ok_or_else(ProductionError::source)?
                            .display
                            .clone();
                        self.unit(Unit {
                            identity: SourceIdentity::new(source_id, loc.line, "say"),
                            kind: ProductionKind::Say,
                            declaration: declaration.clone(),
                            root: root.clone(),
                            source,
                            speaker: Some(ProductionSpeaker { target, display }),
                            direction: say.direction.clone(),
                            controls: controls.to_vec(),
                        })?;
                    }
                }
                Stmt::Text(_) if self.request.include_narration => self.unit(Unit {
                    identity: SourceIdentity::new(source_id, loc.line, "text"),
                    kind: ProductionKind::Text,
                    declaration: declaration.clone(),
                    root: root.clone(),
                    source,
                    speaker: None,
                    direction: None,
                    controls: controls.to_vec(),
                })?,
                Stmt::Choice(choice) => {
                    let mut control = self.control("choice", source.clone(), None);
                    control.once = choice.once;
                    if choice.cond.is_some() {
                        control.condition =
                            Some(self.expression(file, loc.line, ExpressionSlot::Condition(0))?);
                    }
                    if choice.enable.is_some() {
                        control.enable =
                            Some(self.expression(file, loc.line, ExpressionSlot::Enable)?);
                    }
                    let mut next = controls.to_vec();
                    next.push(control);
                    if self.request.include_choices {
                        self.unit(Unit {
                            identity: SourceIdentity::new(source_id, loc.line, "choice"),
                            kind: ProductionKind::Choice,
                            declaration: declaration.clone(),
                            root: root.clone(),
                            source,
                            speaker: None,
                            direction: None,
                            controls: next.clone(),
                        })?;
                    }
                    self.body(&choice.body, owner, declaration, root, &next, depth + 1)?;
                }
                Stmt::If(condition) => {
                    for (index, (expression, body)) in condition.branches.iter().enumerate() {
                        let (physical, span) = self
                            .compiled
                            .program
                            .source_provenance
                            .branch_headers
                            .get(&(file.into(), loc.line, index))
                            .ok_or_else(ProductionError::source)?;
                        let mut control = self.control(
                            if expression.is_some() { "if" } else { "else" },
                            self.source(physical, Loc::new(span.line, span.column))?,
                            None,
                        );
                        if expression.is_some() {
                            control.condition = Some(self.expression(
                                file,
                                loc.line,
                                ExpressionSlot::Condition(index as u32),
                            )?);
                        }
                        let mut next = controls.to_vec();
                        next.push(control);
                        self.body(body, owner, declaration, root, &next, depth + 1)?;
                    }
                }
                Stmt::Scene(scene) => {
                    let target =
                        TargetRef::new("scene", &format!("{}.{}", declaration.id, scene.name));
                    self.add_definition(&target)?;
                    let mut next = controls.to_vec();
                    next.push(self.control("scene", source, Some(target.clone())));
                    self.body(&scene.body, owner, &target, root, &next, depth + 1)?;
                }
                Stmt::Call(call) => {
                    let key = (source_id, source.line, source.column);
                    if let Some(previous) = self.calls.get(&key) {
                        if previous.caller != *declaration
                            || previous.callee.id != call.name
                            || previous.source != source
                            || previous.control_ancestry != controls
                        {
                            return Err(ProductionError::source());
                        }
                    } else {
                        if self.calls.len() >= self.request.limits.call_sites {
                            return Err(ProductionError::budget());
                        }
                        let value = ProductionCallUse {
                            caller: declaration.clone(),
                            callee: TargetRef::new("fragment", &call.name),
                            source,
                            control_ancestry: controls.to_vec(),
                        };
                        self.reserve(&value)?;
                        self.new_callees.insert(value.callee.clone());
                        self.calls.insert(key, value);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn unit(&mut self, unit: Unit) -> Result<(), ProductionError> {
        let key = (unit.identity.clone(), unit.source.column);
        if let Some(previous) = self.units.get(&key) {
            if previous.source != unit.source
                || previous.declaration != unit.declaration
                || previous.speaker != unit.speaker
                || previous.direction != unit.direction
                || previous.controls != unit.controls
            {
                return Err(ProductionError::source());
            }
            return Ok(());
        }
        if self.units.len() >= self.request.limits.rows {
            return Err(ProductionError::budget());
        }
        self.reserve(&unit)?;
        self.units.insert(key, unit);
        Ok(())
    }
    fn control(
        &self,
        kind: &str,
        source: ProductionSource,
        target: Option<TargetRef>,
    ) -> ProductionControl {
        ProductionControl {
            kind: kind.into(),
            source,
            condition: None,
            enable: None,
            once: false,
            target,
            evaluated: false,
        }
    }
    fn expression(
        &self,
        file: &str,
        line: u32,
        slot: ExpressionSlot,
    ) -> Result<String, ProductionError> {
        let expression = self
            .compiled
            .program
            .source_provenance
            .expression(file, line, slot)
            .ok_or_else(ProductionError::source)?;
        let span = expression.span.ok_or_else(ProductionError::source)?;
        let path = Path::new(&expression.file);
        let text = self
            .compiled
            .sources
            .get(path)
            .ok_or_else(ProductionError::source)?;
        let offsets = self.offsets.get(path).ok_or_else(ProductionError::source)?;
        let index = span
            .line
            .checked_sub(1)
            .ok_or_else(ProductionError::source)? as usize;
        let start = *offsets.get(index).ok_or_else(ProductionError::source)?;
        let end = offsets.get(index + 1).copied().unwrap_or(text.len());
        let line = text[start..end].trim_end_matches(['\r', '\n']);
        let column = span
            .column
            .checked_sub(1)
            .ok_or_else(ProductionError::source)? as usize;
        let boundary = |index| {
            line.char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(line.len()))
                .nth(index)
                .ok_or_else(ProductionError::source)
        };
        Ok(line[boundary(column)?..boundary(column + span.length as usize)?].into())
    }
}
