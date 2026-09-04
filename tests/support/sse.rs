//! SSE chunk planning: controls how SSE data lines are split across TCP body
//! chunks to test the gateway's streaming reassembly.

use bytes::Bytes;
use std::time::Duration;

/// Describes how to split a raw SSE body into TCP-level body chunks.
#[derive(Debug, Clone)]
pub struct SseChunkPlan {
    /// Strategy for splitting the body into chunks.
    pub strategy: ChunkStrategy,
    /// Optional delay between chunks (simulates network jitter / slow upstream).
    pub idle_delay: Option<Duration>,
}

/// Strategy for splitting SSE body bytes into chunks.
#[derive(Debug, Clone)]
pub enum ChunkStrategy {
    /// Deliver the entire body as a single chunk.
    SingleChunk,
    /// Split at every `\n\n` boundary (one SSE event per chunk).
    PerEvent,
    /// Split each SSE `data:` line into N fragments, simulating mid-line TCP splits.
    SplitDataLines {
        /// Number of fragments per data line (2 or 3).
        fragments_per_line: usize,
    },
    /// Merge multiple SSE events into a single chunk, then emit the rest normally.
    MergeEvents {
        /// Number of events to merge into the first chunk.
        merge_count: usize,
    },
    /// Provide explicit chunk boundaries (byte offsets into the body).
    ExplicitOffsets(Vec<usize>),
}

impl SseChunkPlan {
    /// One event per chunk, no artificial delay.
    pub fn per_event() -> Self {
        Self {
            strategy: ChunkStrategy::PerEvent,
            idle_delay: None,
        }
    }

    /// Single chunk, no splitting.
    pub fn single_chunk() -> Self {
        Self {
            strategy: ChunkStrategy::SingleChunk,
            idle_delay: None,
        }
    }

    /// Split each `data:` line into N fragments.
    pub fn split_data_lines(fragments: usize) -> Self {
        Self {
            strategy: ChunkStrategy::SplitDataLines {
                fragments_per_line: fragments,
            },
            idle_delay: None,
        }
    }

    /// Merge first N events into one chunk.
    pub fn merge_events(count: usize) -> Self {
        Self {
            strategy: ChunkStrategy::MergeEvents { merge_count: count },
            idle_delay: None,
        }
    }

    /// Set inter-chunk delay.
    #[allow(dead_code)]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.idle_delay = Some(delay);
        self
    }

    /// Generate chunk bytes from the raw body according to the strategy.
    pub fn generate_chunks(&self, body: &str) -> Vec<Bytes> {
        let bytes = body.as_bytes();
        match &self.strategy {
            ChunkStrategy::SingleChunk => {
                vec![Bytes::copy_from_slice(bytes)]
            }
            ChunkStrategy::PerEvent => split_per_event(bytes),
            ChunkStrategy::SplitDataLines { fragments_per_line } => {
                split_data_lines(bytes, *fragments_per_line)
            }
            ChunkStrategy::MergeEvents { merge_count } => merge_first_events(bytes, *merge_count),
            ChunkStrategy::ExplicitOffsets(offsets) => split_at_offsets(bytes, offsets),
        }
    }
}

/// Split at `\n\n` boundaries — each SSE event becomes a separate chunk.
fn split_per_event(body: &[u8]) -> Vec<Bytes> {
    let text = std::str::from_utf8(body).unwrap_or("");
    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        match remaining.find("\n\n") {
            Some(pos) => {
                let end = pos + 2;
                chunks.push(Bytes::copy_from_slice(&remaining.as_bytes()[..end]));
                remaining = &remaining[end..];
            }
            None => {
                chunks.push(Bytes::copy_from_slice(remaining.as_bytes()));
                break;
            }
        }
    }
    chunks
}

/// Split each `data: ...` line into N fragments at roughly equal byte offsets.
fn split_data_lines(body: &[u8], fragments: usize) -> Vec<Bytes> {
    let text = std::str::from_utf8(body).unwrap_or("");
    let mut chunks = Vec::new();
    let fragments = fragments.max(2);

    for line in text.split_inclusive('\n') {
        if line.starts_with("data:") && line.len() > 10 {
            let step = line.len() / fragments;
            let mut start = 0;
            for i in 1..fragments {
                let end = (step * i).min(line.len());
                chunks.push(Bytes::copy_from_slice(&line.as_bytes()[start..end]));
                start = end;
            }
            if start < line.len() {
                chunks.push(Bytes::copy_from_slice(&line.as_bytes()[start..]));
            }
        } else {
            chunks.push(Bytes::copy_from_slice(line.as_bytes()));
        }
    }
    chunks
}

/// Merge first N events into one chunk, then emit remaining events individually.
fn merge_first_events(body: &[u8], merge_count: usize) -> Vec<Bytes> {
    let text = std::str::from_utf8(body).unwrap_or("");
    let events = split_per_event(text.as_bytes());

    let mut chunks = Vec::new();
    let merge_count = merge_count.min(events.len());

    if merge_count > 0 {
        let mut merged = Vec::new();
        for event in &events[..merge_count] {
            merged.extend_from_slice(event);
        }
        chunks.push(Bytes::from(merged));
    }

    for event in &events[merge_count..] {
        chunks.push(event.clone());
    }

    chunks
}

/// Split at explicit byte offsets.
fn split_at_offsets(body: &[u8], offsets: &[usize]) -> Vec<Bytes> {
    let mut chunks = Vec::new();
    let mut start = 0;

    for &offset in offsets {
        let end = offset.min(body.len());
        if end > start {
            chunks.push(Bytes::copy_from_slice(&body[start..end]));
            start = end;
        }
    }
    if start < body.len() {
        chunks.push(Bytes::copy_from_slice(&body[start..]));
    }
    chunks
}

// ---------------------------------------------------------------------------
// SSE event builder helpers
// ---------------------------------------------------------------------------

/// Build a well-formed SSE frame: `event: {event}\ndata: {data}\n\n`
#[allow(dead_code)]
pub fn sse_event(event: &str, data: &str) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

/// Build a data-only SSE frame: `data: {data}\n\n`
#[allow(dead_code)]
pub fn sse_data(data: &str) -> String {
    format!("data: {data}\n\n")
}

/// OpenAI Chat Completions `[DONE]` terminator.
#[allow(dead_code)]
pub fn sse_done() -> String {
    "data: [DONE]\n\n".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_event_splits_correctly() {
        let body = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: [DONE]\n\n";
        let plan = SseChunkPlan::per_event();
        let chunks = plan.generate_chunks(body);
        assert_eq!(chunks.len(), 3);
        assert_eq!(&chunks[0][..], b"data: {\"a\":1}\n\n");
        assert_eq!(&chunks[1][..], b"data: {\"b\":2}\n\n");
        assert_eq!(&chunks[2][..], b"data: [DONE]\n\n");
    }

    #[test]
    fn split_data_lines_into_fragments() {
        let body = "data: {\"content\":\"hello world test\"}\n\n";
        let plan = SseChunkPlan::split_data_lines(3);
        let chunks = plan.generate_chunks(body);
        assert!(
            chunks.len() >= 3,
            "expected >=3 chunks, got {}",
            chunks.len()
        );
        let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.to_vec()).collect();
        assert_eq!(reassembled, body.as_bytes());
    }

    #[test]
    fn merge_events_combines_first_n() {
        let body = "data: {\"a\":1}\n\ndata: {\"b\":2}\n\ndata: {\"c\":3}\n\n";
        let plan = SseChunkPlan::merge_events(2);
        let chunks = plan.generate_chunks(body);
        assert_eq!(chunks.len(), 2);
        assert_eq!(&chunks[0][..], b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(&chunks[1][..], b"data: {\"c\":3}\n\n");
    }

    #[test]
    fn single_chunk_returns_whole_body() {
        let body = "data: test\n\ndata: [DONE]\n\n";
        let plan = SseChunkPlan::single_chunk();
        let chunks = plan.generate_chunks(body);
        assert_eq!(chunks.len(), 1);
        assert_eq!(&chunks[0][..], body.as_bytes());
    }

    #[test]
    fn explicit_offsets_split_correctly() {
        let body = "abcdefghij";
        let plan = SseChunkPlan {
            strategy: ChunkStrategy::ExplicitOffsets(vec![3, 7]),
            idle_delay: None,
        };
        let chunks = plan.generate_chunks(body);
        assert_eq!(chunks.len(), 3);
        assert_eq!(&chunks[0][..], b"abc");
        assert_eq!(&chunks[1][..], b"defg");
        assert_eq!(&chunks[2][..], b"hij");
    }

    #[test]
    fn reassembly_is_lossless() {
        let body = "event: delta\ndata: {\"x\":1}\n\nevent: done\ndata: {}\n\n";
        for plan in [
            SseChunkPlan::per_event(),
            SseChunkPlan::single_chunk(),
            SseChunkPlan::split_data_lines(2),
            SseChunkPlan::merge_events(1),
        ] {
            let chunks = plan.generate_chunks(body);
            let reassembled: Vec<u8> = chunks.iter().flat_map(|c| c.to_vec()).collect();
            assert_eq!(
                reassembled,
                body.as_bytes(),
                "lossless reassembly failed for {:?}",
                plan.strategy
            );
        }
    }
}
