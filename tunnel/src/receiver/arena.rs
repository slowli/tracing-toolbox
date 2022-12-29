//! Simple string arena.

use once_cell::sync::{Lazy, OnceCell};
use tracing_core::{field::FieldSet, Callsite, Interest, Kind, Level, Metadata};

use std::{
    borrow::Cow,
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    hash::{Hash, Hasher},
    ops,
    sync::RwLock,
};

use crate::types::{CallSiteData, CallSiteKind, TracingLevel};

// An emulation of a hash map with keys equivalent to `CallSiteData` (obviously,
// we don't want to store `CallSiteData` explicitly because of its size).
type MetadataMap = HashMap<u64, Vec<&'static Metadata<'static>>>;

impl From<TracingLevel> for Level {
    fn from(level: TracingLevel) -> Self {
        match level {
            TracingLevel::Error => Self::ERROR,
            TracingLevel::Warn => Self::WARN,
            TracingLevel::Info => Self::INFO,
            TracingLevel::Debug => Self::DEBUG,
            TracingLevel::Trace => Self::TRACE,
        }
    }
}

impl From<CallSiteKind> for Kind {
    fn from(kind: CallSiteKind) -> Self {
        match kind {
            CallSiteKind::Span => Self::SPAN,
            CallSiteKind::Event => Self::EVENT,
        }
    }
}

#[derive(Debug, Default)]
struct DynamicCallSite {
    metadata: OnceCell<&'static Metadata<'static>>,
}

impl Callsite for DynamicCallSite {
    fn set_interest(&self, _interest: Interest) {
        // Does nothing
    }

    fn metadata(&self) -> &Metadata<'_> {
        self.metadata
            .get()
            .copied()
            .expect("metadata not initialized")
    }
}

#[derive(Debug, Default)]
pub(crate) struct Arena {
    strings: RwLock<HashSet<&'static str>>,
    metadata: RwLock<MetadataMap>,
}

impl Arena {
    fn leak(s: Cow<'static, str>) -> &'static str {
        match s {
            Cow::Borrowed(s) => s,
            Cow::Owned(string) => Box::leak(string.into_boxed_str()),
        }
    }

    fn new_call_site() -> &'static DynamicCallSite {
        let call_site = Box::default();
        Box::leak(call_site)
    }

    fn lock_strings(&self) -> impl ops::Deref<Target = HashSet<&'static str>> + '_ {
        self.strings.read().unwrap()
    }

    fn lock_strings_mut(&self) -> impl ops::DerefMut<Target = HashSet<&'static str>> + '_ {
        self.strings.write().unwrap()
    }

    fn alloc_string(&self, s: Cow<'static, str>) -> &'static str {
        if let Some(existing) = self.lock_strings().get(s.as_ref()).copied() {
            return existing;
        }

        let mut lock = self.lock_strings_mut();
        if let Some(existing) = lock.get(s.as_ref()).copied() {
            return existing;
        }
        let leaked = Self::leak(s);
        lock.insert(leaked);
        leaked
    }

    fn leak_fields(&self, fields: Vec<Cow<'static, str>>) -> &'static [&'static str] {
        let fields: Box<[_]> = fields
            .into_iter()
            .map(|field| self.alloc_string(field))
            .collect();
        Box::leak(fields)
    }

    fn leak_metadata(&self, data: CallSiteData) -> &'static Metadata<'static> {
        let call_site = Self::new_call_site();
        let call_site_id = tracing_core::identify_callsite!(call_site);
        let fields = FieldSet::new(self.leak_fields(data.fields), call_site_id);
        let metadata = Metadata::new(
            self.alloc_string(data.name),
            self.alloc_string(data.target),
            data.level.into(),
            data.file.map(|file| self.alloc_string(file)),
            data.line,
            data.module_path.map(|path| self.alloc_string(path)),
            fields,
            data.kind.into(),
        );

        let metadata = Box::leak(Box::new(metadata)) as &_;
        call_site.metadata.set(metadata).unwrap();
        metadata
    }

    fn lock_metadata(&self) -> impl ops::Deref<Target = MetadataMap> + '_ {
        self.metadata.read().unwrap()
    }

    fn lock_metadata_mut(&self) -> impl ops::DerefMut<Target = MetadataMap> + '_ {
        self.metadata.write().unwrap()
    }

    /// Returns the metadata and a flag whether it was allocated in this call.
    pub(super) fn alloc_metadata(&self, data: CallSiteData) -> (&'static Metadata<'static>, bool) {
        let hash_value = Self::hash_metadata(&data);
        let scanned_bucket_len = {
            let lock = self.lock_metadata();
            if let Some(bucket) = lock.get(&hash_value) {
                for &metadata in bucket {
                    if Self::eq_metadata(&data, metadata) {
                        return (metadata, false);
                    }
                }
                bucket.len()
            } else {
                0
            }
        };

        let mut lock = self.lock_metadata_mut();
        let bucket = lock.entry(hash_value).or_default();
        for &metadata in &bucket[scanned_bucket_len..] {
            if Self::eq_metadata(&data, metadata) {
                return (metadata, false);
            }
        }

        // Finally, we need to actually leak metadata.
        let metadata = self.leak_metadata(data);
        bucket.push(metadata);
        (metadata, true)
    }

    // The returned hash doesn't necessarily match the hash of `Metadata`, but it is the same
    // for the equivalent `(kind, data)` tuples, which is what we need.
    fn hash_metadata(data: &CallSiteData) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    fn eq_metadata(data: &CallSiteData, metadata: &Metadata<'_>) -> bool {
        // number comparisons go first
        matches!(data.kind, CallSiteKind::Span) == metadata.is_span()
            && Level::from(data.level) == *metadata.level()
            && data.line == metadata.line()
            // ...then, string comparisons
            && data.name == metadata.name()
            && data.target == metadata.target()
            && data.module_path.as_ref().map(Cow::as_ref) == metadata.module_path()
            && data.file.as_ref().map(Cow::as_ref) == metadata.file()
            // ...and finally, comparison of fields
            && data
                .fields
                .iter()
                .map(Cow::as_ref)
                .eq(metadata.fields().iter().map(|field| field.name()))
    }
}

pub(crate) static ARENA: Lazy<Arena> = Lazy::new(Arena::default);
