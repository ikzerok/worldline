//! 在检查点复制前借用计数；不限制解释器单句求值的内部临时值。
use super::{encoded_size, RouteComparisonError};
use crate::{
    AnchorRecord, ChoiceCoverage, Frame, FrameSrc, ReplayCheckpoint, StateRecord, Story, Value,
    REPLAY_SCHEMA_VERSION,
};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Write};

/// 同时覆盖 save 的 pretty 暂存和最终嵌套 state 字符串，之后才可构建检查点。
pub(crate) fn check_checkpoint(
    story: &Story<'_>,
    maximum: usize,
) -> Result<(), RouteComparisonError> {
    let mut state = BorrowedSave::new(story);
    let mut pretty = Counter { used: 0, maximum };
    serde_json::to_writer_pretty(&mut pretty, &state).map_err(|_| limit_error())?;

    // 使用真实外壳模型，state 的两端引号已包含在此；不复制任何故事字符串。
    let wrapper = ReplayCheckpoint {
        presentation: story.presentation_identity().cloned(),
        schema_version: REPLAY_SCHEMA_VERSION,
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        fingerprint: story.fingerprint,
        seed: story.seed,
        state: String::new(),
    };
    let mut escaped = EscapedCounter(Counter {
        used: encoded_size(&wrapper, maximum)?,
        maximum,
    });
    if let Some(pause) = story
        .paused
        .as_ref()
        .filter(|_| story.presentation.is_none())
    {
        state.rng = pause.rng_before;
    }
    state.vars.checkpoint = true;
    state.frames.checkpoint = true;
    serde_json::to_writer(&mut escaped, &state).map_err(|_| limit_error())?;
    Ok(())
}

fn limit_error() -> RouteComparisonError {
    RouteComparisonError::new("output_limit", "比较检查点超过输出字节额度")
}

struct Counter {
    used: usize,
    maximum: usize,
}
impl Counter {
    fn include(&mut self, bytes: usize) -> io::Result<()> {
        let next = self
            .used
            .checked_add(bytes)
            .filter(|next| *next <= self.maximum)
            .ok_or_else(|| io::Error::other("比较检查点超过输出字节额度"))?;
        self.used = next;
        Ok(())
    }
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.include(bytes.len())?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 把 compact JSON 的每段输出按外层 JSON 字符串的转义长度计数。
struct EscapedCounter(Counter);
impl Write for EscapedCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            let length = match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                0..=31 => 6,
                _ => 1,
            };
            self.0.include(length)?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// 字段及省略规则与 model::SaveState 同步；perms 被其 skip_serializing 永远省略。
#[derive(Serialize)]
struct BorrowedSave<'a, 'p> {
    #[serde(skip_serializing_if = "Option::is_none")]
    presentation: Option<&'a crate::RuntimeLocalizationIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presentation_pause_rng: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    required_features: Vec<&'static str>,
    fingerprint: u64,
    vars: BorrowedValues<'a, HashMap<String, Value>>,
    visits: &'a HashMap<String, u32>,
    turns: u32,
    taken_once: &'a [String],
    frames: BorrowedFrames<'a, 'p>,
    glue_pending: bool,
    paused: bool,
    rng: u64,
    seed: u64,
    choice_coverage: &'a BTreeMap<String, ChoiceCoverage>,
    storyline: &'a str,
    // save 的 met_list 排序不改变计数；这里无需构造 Vec 或克隆名字。
    met: &'a HashSet<String>,
    anchors: &'a [AnchorRecord],
    states: &'a BTreeMap<String, Vec<String>>,
    state_history: &'a [StateRecord],
}
impl<'a, 'p> BorrowedSave<'a, 'p> {
    fn new(story: &'a Story<'p>) -> Self {
        let mut required_features = Vec::new();
        if story.presentation.is_some() {
            required_features.push(crate::LOCALIZATION_PRESENTATION_CAPABILITY);
        }
        if worldline_core::language::uses_new_features(story.program) {
            required_features.push("runtime.language_1_11.v1");
        }
        if crate::choices::uses_presentation(story.program) {
            required_features.push(crate::CHOICE_PRESENTATION_CAPABILITY);
        }
        Self {
            presentation: story.presentation_identity(),
            presentation_pause_rng: story
                .presentation
                .as_ref()
                .and_then(|_| story.paused.as_ref().map(|pause| pause.rng_before)),
            required_features,
            fingerprint: story.fingerprint,
            vars: BorrowedValues {
                values: &story.vars,
                checkpoint: false,
            },
            visits: &story.visits,
            turns: story.turns,
            taken_once: &story.taken_once,
            frames: BorrowedFrames {
                frames: &story.frames,
                checkpoint: false,
            },
            glue_pending: story.glue_pending,
            paused: story.paused.is_some(),
            rng: story.rng.get(),
            seed: story.seed,
            choice_coverage: &story.choice_coverage,
            storyline: &story.storyline,
            met: &story.met,
            anchors: &story.anchors,
            states: &story.states,
            state_history: &story.state_history,
        }
    }
}

// 帧逐项借用序列化，不建立 FrameSave Vec，也不复制 fragment locals。
struct BorrowedFrames<'a, 'p> {
    frames: &'a [Frame<'p>],
    checkpoint: bool,
}
impl Serialize for BorrowedFrames<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.frames.len()))?;
        for frame in self.frames {
            sequence.serialize_element(&BorrowedFrame {
                fragment: frame.fragment.as_deref(),
                locals: BorrowedValues {
                    values: &frame.locals,
                    checkpoint: self.checkpoint,
                },
                node: frame.node.as_deref(),
                idx: frame.idx,
                src: &frame.src,
            })?;
        }
        sequence.end()
    }
}
#[derive(Serialize)]
struct BorrowedFrame<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment: Option<&'a str>,
    #[serde(skip_serializing_if = "BorrowedValues::is_empty")]
    locals: BorrowedValues<'a, BTreeMap<String, Value>>,
    node: Option<&'a str>,
    idx: usize,
    src: &'a Option<FrameSrc>,
}

struct BorrowedValues<'a, M> {
    values: &'a M,
    checkpoint: bool,
}
impl BorrowedValues<'_, BTreeMap<String, Value>> {
    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
impl<'a, M> Serialize for BorrowedValues<'a, M>
where
    &'a M: IntoIterator<Item = (&'a String, &'a Value)>,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entries = self.values.into_iter();
        let mut map = serializer.serialize_map(entries.size_hint().1)?;
        for (key, value) in entries {
            map.serialize_entry(
                key,
                &BorrowedValue {
                    value,
                    checkpoint: self.checkpoint,
                },
            )?;
        }
        map.end()
    }
}
struct BorrowedValue<'a> {
    value: &'a Value,
    checkpoint: bool,
}
impl Serialize for BorrowedValue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Value::Num(number) = self.value {
            if self.checkpoint {
                // checkpoint 经 serde_json::Value 解析后再输出；默认浮点解析可能改变
                // 末尾数字长度。仅在固定栈缓冲中复现数值标量，仍完全借用大值。
                let mut bytes = [0_u8; 32];
                let length = {
                    let mut output = io::Cursor::new(bytes.as_mut_slice());
                    serde_json::to_writer(&mut output, number)
                        .map_err(serde::ser::Error::custom)?;
                    output.position() as usize
                };
                let number: serde_json::Value =
                    serde_json::from_slice(&bytes[..length]).map_err(serde::ser::Error::custom)?;
                return serializer.serialize_newtype_variant("Value", 0, "Num", &number);
            }
        }
        self.value.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldline_core::{compile_source, compile_source_with_options, CompileOptions};

    fn assert_exact_limit(story: &Story<'_>) {
        let pretty = story.save().unwrap();
        let checkpoint = story.checkpoint().unwrap();
        let final_size = serde_json::to_vec(&checkpoint).unwrap().len();
        let required = pretty.len().max(final_size);
        assert!(check_checkpoint(story, required).is_ok());
        assert_eq!(
            check_checkpoint(story, required - 1).unwrap_err().code,
            "output_limit"
        );
        assert_eq!(story.save().unwrap(), pretty);
    }

    #[test]
    fn checkpoint_preflight_matches_empty_and_escaped_fields() {
        let compiled = compile_source("limit.wl", "event start\n  -> END\n");
        assert!(!compiled.has_errors());
        let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 1).unwrap();
        assert_exact_limit(&story);
        assert_eq!(
            check_checkpoint(&story, 0).unwrap_err().code,
            "output_limit"
        );

        let escaped = "中文😀\"\\\n\r\t\u{8}\u{c}\u{0}\u{1}\u{1f}";
        story
            .vars
            .insert(escaped.into(), Value::Str(escaped.into()));
        story.met.insert(escaped.into());
        story.met.insert("另一个角色".into());
        story.taken_once.push(escaped.into());
        story.storyline = escaped.into();
        story.states.insert(escaped.into(), vec![escaped.into()]);
        let frame = &mut story.frames[0];
        frame.fragment = Some(escaped.into());
        frame.node = Some(escaped.into());
        frame.src = Some(FrameSrc::FragmentCall {
            stmt: 7,
            fragment: 9,
        });
        frame
            .locals
            .insert(escaped.into(), Value::Str(escaped.into()));
        assert_exact_limit(&story);
    }

    #[test]
    fn checkpoint_preflight_counts_pretty_temporary_and_large_locals() {
        let compiled = compile_source("limit.wl", "event start\n  -> END\n");
        let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 1).unwrap();
        story.frames[0]
            .locals
            .insert("many".into(), Value::TagSet(vec![String::new(); 300]));
        let pretty_size = story.save().unwrap().len();
        let final_size = serde_json::to_vec(&story.checkpoint().unwrap())
            .unwrap()
            .len();
        assert!(pretty_size > final_size);
        assert!(check_checkpoint(&story, final_size).is_err());
        assert_exact_limit(&story);

        story.frames[0]
            .locals
            .insert("large".into(), Value::Str("\\\"".repeat(4096)));
        assert!(check_checkpoint(&story, 4096).is_err());
        assert_exact_limit(&story);
    }

    #[test]
    fn checkpoint_preflight_preserves_features_and_pause_rng() {
        let compiled = compile_source_with_options(
            "limit.wl",
            "fragment nested(open: bool)\n  local unlocked: bool = open\n  choice \"进入\" enable unlocked disabled \"未开放\"\n    return\n  choice \"返回\"\n    return\nevent start\n  call nested(false)\n  -> END\n",
            CompileOptions::v1_12(),
        );
        assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
        let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 1).unwrap();
        story.continue_story().unwrap();
        assert!(story.is_paused());
        story.rng.set(u64::MAX);
        story.paused.as_mut().unwrap().rng_before = 1;
        assert_eq!(BorrowedSave::new(&story).required_features.len(), 2);
        assert_exact_limit(&story);
        assert_eq!(story.rng.get(), u64::MAX);
        story.rng.set(1);
        story.paused.as_mut().unwrap().rng_before = u64::MAX;
        assert_exact_limit(&story);
    }

    #[test]
    fn checkpoint_preflight_matches_roundtripped_numeric_scalars() {
        let compiled = compile_source("limit.wl", "event start\n  -> END\n");
        let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 1).unwrap();
        for number in [
            0.0,
            -0.0,
            1.2345678901234568,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::INFINITY,
            f64::NAN,
        ] {
            story.vars.insert("number".into(), Value::Num(number));
            story.frames[0]
                .locals
                .insert("number".into(), Value::Num(number));
            assert_exact_limit(&story);
        }
    }
}
