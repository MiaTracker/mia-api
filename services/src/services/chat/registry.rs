//! In-process hub between a running Exchange and the SSE responses that follow it.
//!
//! The Exchange job calls `publish` only after the matching database write has committed. Clients
//! `subscribe` and get everything published so far (`replay`) plus a live channel, atomically.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use views::conversations::ExchangeEvent;

pub struct ExchangeSubscription {
    pub replay: Vec<ExchangeEvent>,
    /// `None` when the Exchange already published its terminal event.
    pub live: Option<UnboundedReceiver<ExchangeEvent>>,
}

/// State of one running Exchange. A cancel handle will live here later.
struct Entry {
    /// Text so far, indexed by the Exchange-wide block number.
    blocks: Vec<String>,
    /// The `Completed` or `Failed` event, kept until `close` so late joiners still see it.
    terminal: Option<ExchangeEvent>,
    subscribers: Vec<UnboundedSender<ExchangeEvent>>,
}

#[derive(Default)]
pub struct ExchangeRegistry {
    entries: Mutex<HashMap<i32, Arc<Mutex<Entry>>>>,
}

impl ExchangeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a running Exchange. Keeps the existing entry when the id is already open.
    pub fn open(&self, exchange_id: i32) {
        let mut entries = lock(&self.entries);
        entries.entry(exchange_id).or_insert_with(|| {
            Arc::new(Mutex::new(Entry { blocks: Vec::new(), terminal: None, subscribers: Vec::new() }))
        });
    }

    /// `None` when the Exchange is not open.
    pub fn subscribe(&self, exchange_id: i32) -> Option<ExchangeSubscription> {
        let entry = self.entry(exchange_id)?;
        let mut entry = lock(&entry);
        let mut replay: Vec<ExchangeEvent> = entry
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, text)| !text.is_empty())
            .map(|(block, text)| ExchangeEvent::TextDelta { block: block as u32, text: text.clone() })
            .collect();
        if let Some(terminal) = &entry.terminal {
            replay.push(terminal.clone());
            return Some(ExchangeSubscription { replay, live: None });
        }
        let (sender, receiver) = unbounded_channel();
        entry.subscribers.push(sender);
        Some(ExchangeSubscription { replay, live: Some(receiver) })
    }

    /// Call only after the database write behind `event` has committed. No-op when not open.
    pub fn publish(&self, exchange_id: i32, event: ExchangeEvent) {
        let Some(entry) = self.entry(exchange_id) else { return };
        let mut entry = lock(&entry);
        match &event {
            ExchangeEvent::TextDelta { block, text } => {
                let block = *block as usize;
                if entry.blocks.len() <= block {
                    entry.blocks.resize(block + 1, String::new());
                }
                entry.blocks[block].push_str(text);
            }
            ExchangeEvent::Completed { .. } | ExchangeEvent::Failed { .. } => entry.terminal = Some(event.clone())
        }
        entry.subscribers.retain(|subscriber| subscriber.send(event.clone()).is_ok());
    }

    /// Drops all senders, which ends the live streams. No-op when not open.
    pub fn close(&self, exchange_id: i32) {
        lock(&self.entries).remove(&exchange_id);
    }

    fn entry(&self, exchange_id: i32) -> Option<Arc<Mutex<Entry>>> {
        lock(&self.entries).get(&exchange_id).cloned()
    }
}

/// A panic while holding the lock cannot leave the data half-updated in a harmful way, so ignore poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use tokio::sync::mpsc::error::TryRecvError;

    fn delta(block: u32, text: &str) -> ExchangeEvent {
        ExchangeEvent::TextDelta { block, text: text.into() }
    }

    #[test]
    fn registry_is_send_and_sync_when_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ExchangeRegistry>();
    }

    #[test]
    fn subscribe_returns_none_when_exchange_not_open() {
        test_support::init();
        let registry = ExchangeRegistry::new();

        assert!(registry.subscribe(1).is_none());
    }

    #[test]
    fn subscribe_replays_published_text_per_block_when_joining_late() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        registry.publish(1, delta(0, "Hel"));
        registry.publish(1, delta(0, "lo"));
        registry.publish(1, delta(1, "!"));

        let subscription = registry.subscribe(1).unwrap();

        assert_eq!(subscription.replay, vec![delta(0, "Hello"), delta(1, "!")]);
        assert!(subscription.live.is_some());
    }

    #[test]
    fn subscribe_skips_empty_blocks_when_replaying_with_a_gap() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        registry.publish(1, delta(2, "x"));

        let subscription = registry.subscribe(1).unwrap();

        assert_eq!(subscription.replay, vec![delta(2, "x")]);
    }

    #[test]
    fn publish_reaches_live_subscriber_after_replay_without_duplicates() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        registry.publish(1, delta(0, "a"));
        let mut subscription = registry.subscribe(1).unwrap();

        registry.publish(1, delta(0, "b"));

        let mut live = subscription.live.take().unwrap();
        assert_eq!(subscription.replay, vec![delta(0, "a")]);
        assert_eq!(live.try_recv().unwrap(), delta(0, "b"));
        assert!(matches!(live.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn publish_skips_dropped_subscriber() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        let dropped = registry.subscribe(1).unwrap();
        let mut kept = registry.subscribe(1).unwrap().live.unwrap();
        drop(dropped);

        registry.publish(1, delta(0, "a"));

        assert_eq!(kept.try_recv().unwrap(), delta(0, "a"));
    }

    #[test]
    fn publish_does_nothing_when_exchange_not_open() {
        test_support::init();
        let registry = ExchangeRegistry::new();

        registry.publish(1, delta(0, "a"));

        assert!(registry.subscribe(1).is_none());
    }

    #[test]
    fn open_keeps_existing_entry_when_already_open() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        registry.publish(1, delta(0, "a"));

        registry.open(1);

        assert_eq!(registry.subscribe(1).unwrap().replay, vec![delta(0, "a")]);
    }

    #[test]
    fn close_ends_live_stream() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        let mut live = registry.subscribe(1).unwrap().live.unwrap();
        registry.publish(1, delta(0, "a"));

        registry.close(1);

        assert_eq!(live.try_recv().unwrap(), delta(0, "a"));
        assert!(matches!(live.try_recv(), Err(TryRecvError::Disconnected)));
        assert!(registry.subscribe(1).is_none());
    }

    #[test]
    fn close_does_nothing_when_exchange_not_open() {
        test_support::init();
        let registry = ExchangeRegistry::new();

        registry.close(1);

        assert!(registry.subscribe(1).is_none());
    }

    #[test]
    fn subscribe_replays_terminal_event_when_joining_after_completion() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        registry.publish(1, delta(0, "Hi"));
        registry.publish(1, ExchangeEvent::Completed { exchange_id: 1 });

        let subscription = registry.subscribe(1).unwrap();

        assert_eq!(subscription.replay, vec![delta(0, "Hi"), ExchangeEvent::Completed { exchange_id: 1 }]);
        assert!(subscription.live.is_none());
    }

    #[test]
    fn publish_delivers_terminal_event_to_live_subscriber_when_exchange_fails() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(1);
        let mut live = registry.subscribe(1).unwrap().live.unwrap();
        let failed = ExchangeEvent::Failed { exchange_id: 1, key: "Internal".into(), debug_message: "boom".into() };

        registry.publish(1, failed.clone());

        assert_eq!(live.try_recv().unwrap(), failed);
    }
}
