//! Bounded inspection of multipart framing before a request reaches the upstream.
use pragma_domain::{IngressPolicy, MultipartMetrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    BodyLimit,
    MalformedMultipart,
    MultipartPartLimit,
    MultipartHeaderLimit,
    MultipartNestingLimit,
    MultipartFileLimit,
}

impl GuardError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::BodyLimit => "body_limit",
            Self::MalformedMultipart => "malformed_multipart",
            Self::MultipartPartLimit => "multipart_part_limit",
            Self::MultipartHeaderLimit => "multipart_header_limit",
            Self::MultipartNestingLimit => "multipart_nesting_limit",
            Self::MultipartFileLimit => "multipart_file_limit",
        }
    }
}

fn boundary(content_type: &str) -> Result<Option<Vec<u8>>, GuardError> {
    let mut values = content_type.split(';');
    let media = values.next().unwrap_or("").trim();
    if !media.eq_ignore_ascii_case("multipart/form-data") {
        return Ok(None);
    }
    let mut found = None;
    for value in values {
        let Some((key, raw)) = value.trim().split_once('=') else {
            return Err(GuardError::MalformedMultipart);
        };
        if key.trim().eq_ignore_ascii_case("boundary") {
            if found.is_some() {
                return Err(GuardError::MalformedMultipart);
            }
            let raw = raw.trim();
            let token = raw
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(raw);
            if token.is_empty()
                || token.len() > 70
                || !token
                    .bytes()
                    .all(|b| (33..=126).contains(&b) && b != b'"' && b != b'\\')
            {
                return Err(GuardError::MalformedMultipart);
            }
            found = Some(token.as_bytes().to_vec());
        }
    }
    found.map(Some).ok_or(GuardError::MalformedMultipart)
}

fn disposition_name(value: &str) -> Result<(String, bool), GuardError> {
    let mut items = value.split(';');
    if !items
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("form-data")
    {
        return Err(GuardError::MalformedMultipart);
    }
    let mut name = None;
    let mut file = false;
    for item in items {
        let Some((key, raw)) = item.trim().split_once('=') else {
            return Err(GuardError::MalformedMultipart);
        };
        let raw = raw.trim();
        let value = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(raw);
        if key.trim().eq_ignore_ascii_case("name") {
            if name.replace(value.to_owned()).is_some() {
                return Err(GuardError::MalformedMultipart);
            }
        } else if key.trim().eq_ignore_ascii_case("filename") {
            file = true;
        }
    }
    name.filter(|n| !n.is_empty())
        .map(|n| (n, file))
        .ok_or(GuardError::MalformedMultipart)
}

fn field_depth(name: &str) -> Result<usize, GuardError> {
    let mut depth = 0;
    let mut open = false;
    for ch in name.chars() {
        if ch == '[' {
            if open {
                return Err(GuardError::MalformedMultipart);
            }
            open = true;
            depth += 1;
        } else if ch == ']' {
            if !open {
                return Err(GuardError::MalformedMultipart);
            }
            open = false;
        }
    }
    if open {
        return Err(GuardError::MalformedMultipart);
    }
    Ok(depth)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Opaque,
    Start,
    Headers,
    Body,
    Done,
}

fn failure_table(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0; pattern.len()];
    let mut matched = 0;
    for i in 1..pattern.len() {
        while matched > 0 && pattern[i] != pattern[matched] {
            matched = failure[matched - 1];
        }
        if pattern[i] == pattern[matched] {
            matched += 1;
        }
        failure[i] = matched;
    }
    failure
}

fn advance(pattern: &[u8], failure: &[usize], matched: &mut usize, byte: u8) -> bool {
    while *matched > 0 && byte != pattern[*matched] {
        *matched = failure[*matched - 1];
    }
    if byte == pattern[*matched] {
        *matched += 1;
    }
    *matched == pattern.len()
}

/// Parses decoded body chunks with memory bounded by multipart header and boundary limits.
pub struct IngressInspector {
    policy: IngressPolicy,
    phase: Phase,
    start_pattern: Vec<u8>,
    start: Vec<u8>,
    headers: Vec<u8>,
    next_pattern: Vec<u8>,
    final_pattern: Vec<u8>,
    next_failure: Vec<usize>,
    final_failure: Vec<usize>,
    next_match: usize,
    final_match: usize,
    part_bytes: u64,
    current_file: bool,
    trailer: Vec<u8>,
    total: usize,
    metrics: MultipartMetrics,
}

impl IngressInspector {
    pub fn new(policy: &IngressPolicy, content_type: &str) -> Result<Self, GuardError> {
        let boundary = boundary(content_type)?;
        let mut start_pattern = Vec::new();
        let mut next_pattern = Vec::new();
        let mut final_pattern = Vec::new();
        let phase = if let Some(boundary) = boundary {
            start_pattern.extend_from_slice(b"--");
            start_pattern.extend_from_slice(&boundary);
            next_pattern.extend_from_slice(b"\r\n");
            next_pattern.extend_from_slice(&start_pattern);
            final_pattern.extend_from_slice(&next_pattern);
            next_pattern.extend_from_slice(b"\r\n");
            final_pattern.extend_from_slice(b"--");
            Phase::Start
        } else {
            Phase::Opaque
        };
        let next_failure = failure_table(&next_pattern);
        let final_failure = failure_table(&final_pattern);
        Ok(Self {
            policy: policy.clone(),
            phase,
            start_pattern,
            start: Vec::new(),
            headers: Vec::new(),
            next_pattern,
            final_pattern,
            next_failure,
            final_failure,
            next_match: 0,
            final_match: 0,
            part_bytes: 0,
            current_file: false,
            trailer: Vec::new(),
            total: 0,
            metrics: MultipartMetrics::default(),
        })
    }

    fn finish_headers(&mut self) -> Result<(), GuardError> {
        let header_bytes = &self.headers[..self.headers.len() - 4];
        let mut header_count = 0;
        let mut name = None;
        for line in header_bytes.split(|b| *b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() || line[0].is_ascii_whitespace() {
                return Err(GuardError::MalformedMultipart);
            }
            let colon = line
                .iter()
                .position(|b| *b == b':')
                .ok_or(GuardError::MalformedMultipart)?;
            let (key, value) = (&line[..colon], &line[colon + 1..]);
            let key = std::str::from_utf8(key).map_err(|_| GuardError::MalformedMultipart)?;
            let value = std::str::from_utf8(value)
                .map_err(|_| GuardError::MalformedMultipart)?
                .trim();
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(GuardError::MalformedMultipart);
            }
            header_count += 1;
            if header_count > self.policy.max_headers_per_part {
                return Err(GuardError::MultipartHeaderLimit);
            }
            if key.eq_ignore_ascii_case("content-disposition")
                && name.replace(disposition_name(value)?).is_some()
            {
                return Err(GuardError::MalformedMultipart);
            }
        }
        let (name, is_file) = name.ok_or(GuardError::MalformedMultipart)?;
        let depth = field_depth(&name)?;
        if depth > self.policy.max_field_nesting {
            return Err(GuardError::MultipartNestingLimit);
        }
        self.metrics.parts += 1;
        if self.metrics.parts as usize > self.policy.max_multipart_parts {
            return Err(GuardError::MultipartPartLimit);
        }
        self.metrics.max_part_headers = self.metrics.max_part_headers.max(header_count as u32);
        self.metrics.max_field_nesting = self.metrics.max_field_nesting.max(depth as u32);
        self.metrics.total_header_bytes = self
            .metrics
            .total_header_bytes
            .saturating_add(header_bytes.len() as u32);
        self.current_file = is_file;
        if is_file {
            self.metrics.file_parts += 1;
            if self.metrics.file_parts as usize > self.policy.max_file_parts {
                return Err(GuardError::MultipartFileLimit);
            }
        }
        self.headers.clear();
        self.phase = Phase::Body;
        self.part_bytes = 0;
        self.next_match = 0;
        self.final_match = 0;
        Ok(())
    }

    fn finish_part(&mut self, pattern_len: usize) {
        if self.current_file && self.part_bytes == pattern_len as u64 {
            self.metrics.empty_file_parts += 1;
        }
        self.part_bytes = 0;
        self.current_file = false;
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), GuardError> {
        if chunk.len() > self.policy.max_body_bytes.saturating_sub(self.total) {
            return Err(GuardError::BodyLimit);
        }
        self.total += chunk.len();
        if self.phase == Phase::Opaque {
            return Ok(());
        }
        for &byte in chunk {
            match self.phase {
                Phase::Opaque => unreachable!(),
                Phase::Start => {
                    self.start.push(byte);
                    if self.start.len() == self.start_pattern.len() + 2 {
                        if !self.start.starts_with(&self.start_pattern) {
                            return Err(GuardError::MalformedMultipart);
                        }
                        self.phase = match &self.start[self.start_pattern.len()..] {
                            b"\r\n" => Phase::Headers,
                            b"--" => Phase::Done,
                            _ => return Err(GuardError::MalformedMultipart),
                        };
                        self.start.clear();
                    }
                }
                Phase::Headers => {
                    self.headers.push(byte);
                    if self.headers.len() > self.policy.max_header_bytes_per_part + 4 {
                        return Err(GuardError::MultipartHeaderLimit);
                    }
                    if self.headers.ends_with(b"\r\n\r\n") {
                        if self.headers.len() - 4 > self.policy.max_header_bytes_per_part {
                            return Err(GuardError::MultipartHeaderLimit);
                        }
                        self.finish_headers()?;
                    }
                }
                Phase::Body => {
                    self.part_bytes += 1;
                    let next = advance(
                        &self.next_pattern,
                        &self.next_failure,
                        &mut self.next_match,
                        byte,
                    );
                    let final_part = advance(
                        &self.final_pattern,
                        &self.final_failure,
                        &mut self.final_match,
                        byte,
                    );
                    if next {
                        self.finish_part(self.next_pattern.len());
                        self.phase = Phase::Headers;
                    } else if final_part {
                        self.finish_part(self.final_pattern.len());
                        self.phase = Phase::Done;
                    }
                }
                Phase::Done => {
                    self.trailer.push(byte);
                    if self.trailer.len() > 2 || !b"\r\n".starts_with(&self.trailer) {
                        return Err(GuardError::MalformedMultipart);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Option<MultipartMetrics>, GuardError> {
        match self.phase {
            Phase::Opaque => Ok(None),
            Phase::Done if self.trailer.is_empty() || self.trailer == b"\r\n" => {
                Ok(Some(self.metrics))
            }
            _ => Err(GuardError::MalformedMultipart),
        }
    }
}

pub fn inspect(
    policy: &IngressPolicy,
    content_type: &str,
    body: &[u8],
) -> Result<Option<MultipartMetrics>, GuardError> {
    let mut inspector = IngressInspector::new(policy, content_type)?;
    inspector.feed(body)?;
    inspector.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> IngressPolicy {
        IngressPolicy {
            name: "upload".into(),
            max_body_bytes: 1024,
            max_multipart_parts: 2,
            max_headers_per_part: 2,
            max_header_bytes_per_part: 256,
            max_field_nesting: 1,
            max_file_parts: 1,
        }
    }
    #[test]
    fn counts_and_rejects_malformed_or_excessive_parts() {
        let body = b"--x\r\nContent-Disposition: form-data; name=\"a[b]\"\r\n\r\nhello\r\n--x\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a\"\r\n\r\nhi\r\n--x--\r\n";
        let result = inspect(&policy(), "multipart/form-data; boundary=x", body)
            .unwrap()
            .unwrap();
        assert_eq!(
            (result.parts, result.file_parts, result.max_field_nesting),
            (2, 1, 1)
        );
        assert_eq!(
            inspect(
                &policy(),
                "multipart/form-data; boundary=x",
                &body[..body.len() - 6]
            ),
            Err(GuardError::MalformedMultipart)
        );
        let mut strict = policy();
        strict.max_multipart_parts = 1;
        assert_eq!(
            inspect(&strict, "multipart/form-data; boundary=x", body),
            Err(GuardError::MultipartPartLimit)
        );
    }
    #[test]
    fn incremental_chunks_match_whole_body_and_count_empty_files() {
        let body = b"--x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"x\"\r\n\r\n\r\n--x\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nhello\r\n--xX is body text\r\n--x--\r\n";
        let expected = inspect(&policy(), "multipart/form-data; boundary=x", body)
            .unwrap()
            .unwrap();
        let mut stream =
            IngressInspector::new(&policy(), "multipart/form-data; boundary=x").unwrap();
        for chunk in body.chunks(1) {
            stream.feed(chunk).unwrap();
        }
        assert_eq!(stream.finish().unwrap(), Some(expected));
        assert_eq!((expected.parts, expected.empty_file_parts), (2, 1));
        assert!(expected.total_header_bytes > 80);
        let mut strict = policy();
        strict.max_body_bytes = body.len() - 1;
        let mut stream = IngressInspector::new(&strict, "multipart/form-data; boundary=x").unwrap();
        assert_eq!(stream.feed(body), Err(GuardError::BodyLimit));
    }
}
