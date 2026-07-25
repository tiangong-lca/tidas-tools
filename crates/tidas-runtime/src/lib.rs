//! Bounded execution primitives for large TIDAS data paths.

use std::fmt::Write as _;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, SendTimeoutError, Sender, bounded};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(25);
pub const SPOOL_SUMMARY_SCHEMA_V1: &str = "tidas.spool-summary.v1";
pub const SPOOL_SUMMARY_JSON_SCHEMA_V1: &str =
    include_str!("../../../contracts/spool-summary.v1.schema.json");

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> Result<(), RuntimeError> {
        if self.is_cancelled() {
            Err(RuntimeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemoryBudget {
    inner: Arc<BudgetState>,
}

#[derive(Debug)]
struct BudgetState {
    limit: u64,
    used: AtomicU64,
    peak: AtomicU64,
}

impl MemoryBudget {
    #[must_use]
    pub fn new(limit: u64) -> Self {
        Self {
            inner: Arc::new(BudgetState {
                limit,
                used: AtomicU64::new(0),
                peak: AtomicU64::new(0),
            }),
        }
    }

    pub fn reserve(&self, bytes: u64) -> Result<MemoryReservation, RuntimeError> {
        let mut used = self.inner.used.load(Ordering::Acquire);
        loop {
            let requested = used
                .checked_add(bytes)
                .ok_or(RuntimeError::BudgetExceeded {
                    requested: bytes,
                    used,
                    limit: self.inner.limit,
                })?;
            if requested > self.inner.limit {
                return Err(RuntimeError::BudgetExceeded {
                    requested: bytes,
                    used,
                    limit: self.inner.limit,
                });
            }
            match self.inner.used.compare_exchange_weak(
                used,
                requested,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.inner.peak.fetch_max(requested, Ordering::AcqRel);
                    return Ok(MemoryReservation {
                        state: Arc::clone(&self.inner),
                        bytes,
                    });
                }
                Err(actual) => used = actual,
            }
        }
    }

    #[must_use]
    pub fn used(&self) -> u64 {
        self.inner.used.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn peak(&self) -> u64 {
        self.inner.peak.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn limit(&self) -> u64 {
        self.inner.limit
    }
}

#[derive(Debug)]
pub struct MemoryReservation {
    state: Arc<BudgetState>,
    bytes: u64,
}

impl Drop for MemoryReservation {
    fn drop(&mut self) {
        self.state.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub struct Budgeted<T> {
    value: T,
    _reservation: MemoryReservation,
}

impl<T> Budgeted<T> {
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

#[derive(Clone, Debug)]
pub struct BoundedSender<T> {
    sender: Sender<Budgeted<T>>,
    budget: MemoryBudget,
}

#[derive(Clone, Debug)]
pub struct BoundedReceiver<T> {
    receiver: Receiver<Budgeted<T>>,
}

#[must_use]
pub fn bounded_queue<T>(
    capacity: usize,
    budget: MemoryBudget,
) -> (BoundedSender<T>, BoundedReceiver<T>) {
    let (sender, receiver) = bounded(capacity);
    (
        BoundedSender { sender, budget },
        BoundedReceiver { receiver },
    )
}

impl<T> BoundedSender<T> {
    pub fn send(
        &self,
        value: T,
        estimated_bytes: u64,
        cancellation: &CancellationToken,
    ) -> Result<(), RuntimeError> {
        cancellation.check()?;
        let reservation = self.budget.reserve(estimated_bytes)?;
        let mut message = Budgeted {
            value,
            _reservation: reservation,
        };
        loop {
            cancellation.check()?;
            match self.sender.send_timeout(message, QUEUE_POLL_INTERVAL) {
                Ok(()) => return Ok(()),
                Err(SendTimeoutError::Timeout(returned)) => message = returned,
                Err(SendTimeoutError::Disconnected(_)) => {
                    return Err(RuntimeError::QueueDisconnected);
                }
            }
        }
    }
}

impl<T> BoundedReceiver<T> {
    pub fn recv(&self, cancellation: &CancellationToken) -> Result<Budgeted<T>, RuntimeError> {
        loop {
            cancellation.check()?;
            match self.receiver.recv_timeout(QUEUE_POLL_INTERVAL) {
                Ok(value) => return Ok(value),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(RuntimeError::QueueDisconnected),
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpoolSummaryV1 {
    pub schema_version: String,
    pub event_count: u64,
    pub bytes: u64,
    pub sha256: String,
}

pub struct JsonlSpool<W> {
    writer: W,
    hasher: Sha256,
    event_count: u64,
    bytes: u64,
    max_event_bytes: usize,
}

impl<W: Write> JsonlSpool<W> {
    #[must_use]
    pub fn new(writer: W, max_event_bytes: usize) -> Self {
        Self {
            writer,
            hasher: Sha256::new(),
            event_count: 0,
            bytes: 0,
            max_event_bytes,
        }
    }

    pub fn push<T: Serialize>(&mut self, event: &T) -> Result<(), RuntimeError> {
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        if line.len() > self.max_event_bytes {
            return Err(RuntimeError::EventTooLarge {
                actual: line.len(),
                limit: self.max_event_bytes,
            });
        }
        self.writer.write_all(&line)?;
        self.hasher.update(&line);
        self.event_count += 1;
        self.bytes += u64::try_from(line.len()).map_err(|_| RuntimeError::SizeOverflow)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(W, SpoolSummaryV1), RuntimeError> {
        self.writer.flush()?;
        let summary = SpoolSummaryV1 {
            schema_version: SPOOL_SUMMARY_SCHEMA_V1.to_owned(),
            event_count: self.event_count,
            bytes: self.bytes,
            sha256: digest_hex(self.hasher.finalize().as_slice()),
        };
        Ok((self.writer, summary))
    }
}

fn digest_hex(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("operation cancelled")]
    Cancelled,
    #[error(
        "memory budget exceeded: requested {requested} bytes with {used}/{limit} already reserved"
    )]
    BudgetExceeded {
        requested: u64,
        used: u64,
        limit: u64,
    },
    #[error("bounded queue disconnected")]
    QueueDisconnected,
    #[error("spool event is {actual} bytes, exceeding the {limit}-byte event limit")]
    EventTooLarge { actual: usize, limit: usize },
    #[error("size does not fit the runtime accounting type")]
    SizeOverflow,
    #[error("failed to encode a spool event: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("failed to write a spool event: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn reservations_are_bounded_and_released() {
        let budget = MemoryBudget::new(10);
        let first = budget.reserve(6).unwrap();
        assert!(matches!(
            budget.reserve(5),
            Err(RuntimeError::BudgetExceeded { .. })
        ));
        assert_eq!(budget.used(), 6);
        drop(first);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 6);
    }

    #[test]
    fn queue_releases_memory_when_message_is_dropped() {
        let budget = MemoryBudget::new(32);
        let (sender, receiver) = bounded_queue(1, budget.clone());
        let cancellation = CancellationToken::default();
        sender.send("event", 16, &cancellation).unwrap();
        assert_eq!(budget.used(), 16);
        let message = receiver.recv(&cancellation).unwrap();
        assert_eq!(message.into_inner(), "event");
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn cancellation_interrupts_queue_waits() {
        let budget = MemoryBudget::new(32);
        let (sender, _receiver) = bounded_queue(1, budget);
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            sender.send("event", 8, &cancellation),
            Err(RuntimeError::Cancelled)
        ));
    }

    #[test]
    fn spool_hash_is_repeatable_without_retaining_events() {
        let mut first = JsonlSpool::new(Cursor::new(Vec::new()), 1024);
        let mut second = JsonlSpool::new(Cursor::new(Vec::new()), 1024);
        for index in 0..10_000 {
            let event = serde_json::json!({"index": index, "code": "fixture"});
            first.push(&event).unwrap();
            second.push(&event).unwrap();
        }
        let (_, first_summary) = first.finish().unwrap();
        let (_, second_summary) = second.finish().unwrap();
        assert_eq!(first_summary, second_summary);
        assert_eq!(first_summary.event_count, 10_000);
    }

    #[test]
    fn oversized_spool_event_fails_before_write() {
        let mut spool = JsonlSpool::new(Cursor::new(Vec::new()), 8);
        assert!(matches!(
            spool.push(&serde_json::json!({"too": "large"})),
            Err(RuntimeError::EventTooLarge { .. })
        ));
    }

    #[test]
    fn checked_in_json_schema_matches_the_spool_version() {
        let schema: serde_json::Value = serde_json::from_str(SPOOL_SUMMARY_JSON_SCHEMA_V1).unwrap();
        assert_eq!(
            schema["properties"]["schema_version"]["const"],
            SPOOL_SUMMARY_SCHEMA_V1
        );
        assert_eq!(schema["additionalProperties"], false);
    }
}
