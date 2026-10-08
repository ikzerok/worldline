use serde::Serialize;
use std::io::{self, Write};

pub(super) fn encoded_size(value: &impl Serialize, limit: usize) -> Result<usize, String> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.used = self
                .used
                .checked_add(bytes.len())
                .ok_or_else(|| io::Error::other("额度溢出"))?;
            if self.used > self.limit {
                return Err(io::Error::other("序列化预算超限"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { used: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|e| e.to_string())?;
    Ok(counter.used)
}

/// 两遍流式计量/摘要避免为了判断预算而先构造另一份完整JSON。
pub(super) fn digest(
    request: &impl Serialize,
    request_size: usize,
    plan: &impl Serialize,
    plan_size: usize,
) -> Result<String, String> {
    struct Hasher(u64);
    impl Write for Hasher {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            for &byte in bytes {
                self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut hasher = Hasher(0xcbf29ce484222325);
    let domain = b"worldline-template-plan-v1";
    hasher
        .write_all(&(domain.len() as u64).to_le_bytes())
        .map_err(|e| e.to_string())?;
    hasher.write_all(domain).map_err(|e| e.to_string())?;
    hasher
        .write_all(&(request_size as u64).to_le_bytes())
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut hasher, request).map_err(|e| e.to_string())?;
    hasher
        .write_all(&(plan_size as u64).to_le_bytes())
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut hasher, plan).map_err(|e| e.to_string())?;
    Ok(format!("{:016x}", hasher.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn template_protocol_budget_counts_exact_escaped_utf8_and_digest_bytes() {
        let mut plan = json!({"plan_digest":"","text":"汉字\n\"\\"});
        let empty = serde_json::to_vec(&plan).unwrap().len();
        assert_eq!(encoded_size(&plan, empty).unwrap(), empty);
        assert!(encoded_size(&plan, empty - 1).is_err());
        plan["plan_digest"] = json!("0123456789abcdef");
        assert_eq!(serde_json::to_vec(&plan).unwrap().len(), empty + 16);
        assert!(encoded_size(&plan, empty + 15).is_err());
        assert_eq!(encoded_size(&plan, empty + 16).unwrap(), empty + 16);
    }

    #[test]
    fn template_protocol_streaming_digest_matches_length_delimited_reference() {
        let request = json!({"source":"行\n\"","values":[0,false,""]});
        let plan = json!({"plan_digest":"","applied":false});
        let request_bytes = serde_json::to_vec(&request).unwrap();
        let plan_bytes = serde_json::to_vec(&plan).unwrap();
        let mut expected = 0xcbf29ce484222325_u64;
        for bytes in [
            b"worldline-template-plan-v1".as_slice(),
            &request_bytes,
            &plan_bytes,
        ] {
            for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
                expected = (expected ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
        assert_eq!(
            digest(&request, request_bytes.len(), &plan, plan_bytes.len()).unwrap(),
            format!("{expected:016x}")
        );
        assert_ne!(
            digest(&request, request_bytes.len(), &plan, plan_bytes.len()).unwrap(),
            digest(&plan, plan_bytes.len(), &request, request_bytes.len()).unwrap()
        );
    }
}
