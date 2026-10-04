use super::*;
use crate::Span;

pub(super) struct Sources<'a> {
    result: &'a CompileResult,
    offsets: BTreeMap<String, Vec<usize>>,
}
impl<'a> Sources<'a> {
    pub fn new(result: &'a CompileResult) -> Self {
        let offsets = result
            .sources
            .iter()
            .map(|(path, text)| {
                let mut offsets = vec![0];
                offsets.extend(text.match_indices('\n').map(|(index, _)| index + 1));
                (path.to_string_lossy().into_owned(), offsets)
            })
            .collect();
        Self { result, offsets }
    }
    pub fn location(
        &self,
        target: &TargetRef,
        file: &str,
        span: Span,
    ) -> Result<ReviewSource, ReviewError> {
        let (start, end) = self.range(file, span)?;
        if end - start > MAX_REVIEW_JSON_BYTES
            || file.len() > MAX_REVIEW_JSON_BYTES
            || target.id.len() > MAX_REVIEW_JSON_BYTES
        {
            return Err(ReviewError::limit());
        }
        let text = self
            .result
            .sources
            .get(&PathBuf::from(file))
            .ok_or_else(ReviewError::source)?;
        Ok(ReviewSource {
            target: target.clone(),
            file: file.into(),
            line: span.line,
            column: span.column,
            byte_start: start,
            byte_end: end,
            excerpt: text[start..end].into(),
        })
    }
    pub fn text(&self, file: &str, span: Span) -> Result<String, ReviewError> {
        let (start, end) = self.range(file, span)?;
        let text = self
            .result
            .sources
            .get(&PathBuf::from(file))
            .ok_or_else(ReviewError::source)?;
        Ok(text[start..end].into())
    }
    fn range(&self, file: &str, span: Span) -> Result<(usize, usize), ReviewError> {
        let text = self
            .result
            .sources
            .get(&PathBuf::from(file))
            .ok_or_else(ReviewError::source)?;
        let offsets = self.offsets.get(file).ok_or_else(ReviewError::source)?;
        let index = span.line.checked_sub(1).ok_or_else(ReviewError::source)? as usize;
        let base = *offsets.get(index).ok_or_else(ReviewError::source)?;
        let end = offsets.get(index + 1).copied().unwrap_or(text.len());
        let line = text[base..end].trim_end_matches(['\n', '\r']);
        let column = span.column.checked_sub(1).ok_or_else(ReviewError::source)? as usize;
        let finish = column
            .checked_add(span.length as usize)
            .ok_or_else(ReviewError::source)?;
        let boundary = |n| {
            line.char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(line.len()))
                .nth(n)
        };
        let start = boundary(column).ok_or_else(ReviewError::source)?;
        let finish = boundary(finish).ok_or_else(ReviewError::source)?;
        Ok((base + start, base + finish))
    }
}

pub(super) fn snapshot(result: &CompileResult) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let mut mix = |bytes: &[u8]| {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    mix(b"worldline-author-review-v1");
    mix(&serde_json::to_vec(&result.options).expect("编译选项可序列化"));
    for (path, text) in &result.sources {
        mix(path.to_string_lossy().as_bytes());
        mix(text.as_bytes());
    }
    format!("review-{hash:016x}")
}
