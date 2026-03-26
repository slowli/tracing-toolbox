//! `Mutex`-based synchronization.

use std::{collections::HashSet, sync::Mutex};

use tracing_core::Metadata;

use super::{metadata_id, EventSync};
use crate::{CallSiteData, MetadataId, TracingEvent};

/// Mutex-based [`EventSync`] implementation that can be used by [`TracingEventSender`](super::TracingEventSender).
#[derive(Debug, Default)]
pub struct Synced(Mutex<HashSet<MetadataId>>);

impl EventSync for Synced {
    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
        sender: impl Fn(TracingEvent),
    ) {
        // We treat both trait methods in the same way since they may arrive out of order.
        self.ensure_callsite_registered(metadata, sender);
    }

    /// Ensures that the callsite for the given metadata is registered.
    /// This method is synchronous and prevents race conditions where
    /// `NewSpan` or `NewEvent` events arrive before their `NewCallSite` dependencies.
    fn ensure_callsite_registered(
        &self,
        metadata: &'static Metadata<'static>,
        sender: impl Fn(TracingEvent),
    ) {
        let metadata_id = metadata_id(metadata);

        // Fast path: check if already registered without lock contention
        {
            let registered = self.0.lock().unwrap();
            if registered.contains(&metadata_id) {
                return;
            }
        }

        // Slow path: register the callsite
        let mut registered = self.0.lock().unwrap();

        // Double-check in case another thread registered it while we waited for the lock
        if !registered.contains(&metadata_id) {
            // Send NewCallSite event before marking as registered
            sender(TracingEvent::NewCallSite {
                id: metadata_id,
                data: CallSiteData::from(metadata),
            });
            registered.insert(metadata_id);
        }
    }
}
