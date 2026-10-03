//! 摘要校验借用 DTO 并流式写哈希，不复制整个报告或临时 JSON。
use super::*;
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};
use std::io::{self, Write};

struct Digest(u64);
impl Write for Digest {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn id(id: &str, version: &str) -> String {
    match id.strip_prefix(version).filter(|suffix| suffix.starts_with(":p")) {
        Some(suffix) => format!("0000000000000000{suffix}"),
        None => id.to_owned(),
    }
}
struct Entry<'a>(&'a ProblemEntry, &'a str);
impl Serialize for Entry<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entry = self.0;
        let mut value = serializer.serialize_struct("ProblemEntry", 10)?;
        value.serialize_field("id", &id(&entry.id, self.1))?;
        value.serialize_field("domain", &entry.domain)?;
        value.serialize_field("severity", &entry.severity)?;
        value.serialize_field("code", &entry.code)?;
        value.serialize_field("message", &entry.message)?;
        value.serialize_field("note", &entry.note)?;
        value.serialize_field("suggestion", &entry.suggestion)?;
        value.serialize_field("primary", &entry.primary)?;
        value.serialize_field("related_count", &entry.related_count)?;
        value.serialize_field("text_truncated", &entry.text_truncated)?;
        value.end()
    }
}
struct Entries<'a>(&'a ProblemsReport);
impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.entries.len()))?;
        for entry in &self.0.entries {
            sequence.serialize_element(&Entry(entry, &self.0.report_version))?;
        }
        sequence.end()
    }
}
struct Related<'a>(&'a ProblemsReport);
impl Serialize for Related<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.related.len()))?;
        for (key, locations) in &self.0.related {
            map.serialize_entry(&id(key, &self.0.report_version), locations)?;
        }
        map.end()
    }
}
pub(crate) fn of(report: &ProblemsReport) -> String {
    let mut digest = Digest(0xcbf29ce484222325);
    serde_json::to_writer(
        &mut digest,
        &(
            report.schema_version,
            &report.content_baseline,
            &report.source_observation,
            &report.language_version,
            &report.coverage,
            Entries(report),
            Related(report),
            &report.limits,
            &report.reasons,
        ),
    ).expect("报告可序列化");
    format!("{:016x}", digest.0)
}
