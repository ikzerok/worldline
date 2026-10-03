use super::{RouteComparisonError, MAX_ROUTE_OUTPUT_BYTES, MAX_ROUTE_REPORT_RECORDS};
use crate::{Output, Story};
use serde::Serialize;
use std::io::{self, Write};

struct Counter {
    used: usize,
    maximum: usize,
}
impl Write for Counter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let next = self
            .used
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::other("比较输出字节数溢出"))?;
        if next > self.maximum {
            return Err(io::Error::other("比较输出超过字节额度"));
        }
        self.used = next;
        Ok(buffer.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn encoded_size<T: Serialize + ?Sized>(
    value: &T,
    maximum: usize,
) -> Result<usize, RouteComparisonError> {
    let mut counter = Counter { used: 0, maximum };
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| RouteComparisonError::new("output_limit", "比较数据超过输出字节额度"))?;
    Ok(counter.used)
}

/// 借用真实值预检，不先建立 state_view/SaveState/JSON 大副本。
pub(crate) fn check_story(story: &Story<'_>, maximum: usize) -> Result<(), RouteComparisonError> {
    let records = story
        .vars
        .len()
        .saturating_add(story.states.len())
        .saturating_add(story.visits.len())
        .saturating_add(story.choice_coverage.len())
        .saturating_add(story.state_history.len())
        .saturating_add(story.anchors.len())
        .saturating_add(story.choices().len());
    if records > MAX_ROUTE_REPORT_RECORDS {
        return Err(RouteComparisonError::new(
            "output_limit",
            "比较数据记录数超过限制",
        ));
    }
    let mut remaining = maximum;
    macro_rules! count {
        ($value:expr) => {
            remaining = remaining.saturating_sub(encoded_size($value, remaining)?);
        };
    }
    count!(&story.vars);
    count!(&story.states);
    count!(&story.visits);
    count!(&story.choice_coverage);
    count!(&story.state_history);
    count!(&story.anchors);
    count!(story.choices());
    count!(&story.taken_once);
    count!(&story.met);
    for frame in &story.frames {
        count!(&frame.locals);
    }
    let _ = remaining;
    Ok(())
}

#[derive(Default)]
pub(crate) struct OutputUsage {
    pub count: usize,
    pub bytes: usize,
    pub exhausted: bool,
}
impl OutputUsage {
    pub fn include(&mut self, outputs: &[Output]) -> bool {
        if self.exhausted {
            return false;
        }
        for output in outputs {
            if self.count >= 32768 {
                self.exhausted = true;
                return false;
            }
            let Ok(bytes) = encoded_size(output, MAX_ROUTE_OUTPUT_BYTES.saturating_sub(self.bytes))
            else {
                self.exhausted = true;
                return false;
            };
            self.count += 1;
            self.bytes += bytes;
        }
        true
    }
}
