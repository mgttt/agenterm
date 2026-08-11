//! Small, deterministic, platform-neutral glyph cache primitives.
//!
//! The cache stores caller-owned values. Font discovery, rasterization, and
//! fallback selection remain outside ui-core; callers can therefore cache an
//! Arc or any other platform-produced glyph representation without making
//! this crate depend on a font backend.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// The dimensions and font configuration that make a rasterized glyph valid.
///
/// A caller must advance `font_generation` whenever the selected face,
/// fallback chain, hinting configuration, or another raster setting changes.
/// Pixel size is kept separately so ordinary zoom changes are also isolated.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GlyphCacheKey {
    pub character: char,
    pub pixel_size: u16,
    pub font_generation: u64,
}

impl GlyphCacheKey {
    pub const fn new(character: char, pixel_size: u16, font_generation: u64) -> Self {
        Self {
            character,
            pixel_size,
            font_generation,
        }
    }
}

/// Counters for observing cache behavior without exposing its storage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlyphCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

/// A bounded FIFO cache with deterministic replacement behavior.
///
/// A hit does not change ordering. This is intentionally simpler than a
/// pointer-heavy LRU: glyph working sets are small, and predictable bounded
/// memory plus low implementation risk matter more here than perfect recency.
pub struct GlyphCache<K, V> {
    capacity: usize,
    values: HashMap<K, V>,
    order: VecDeque<K>,
    stats: GlyphCacheStats,
}

impl<K, V> GlyphCache<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            values: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            stats: GlyphCacheStats::default(),
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Looks up a value and records a hit or miss.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if let Some(value) = self.values.get(key) {
            self.stats.hits = self.stats.hits.saturating_add(1);
            Some(value)
        } else {
            self.stats.misses = self.stats.misses.saturating_add(1);
            None
        }
    }

    /// Looks up and clones a value, useful for cheaply cloned handles such as
    /// `Arc<...>` and for cached `Option<Arc<...>>` misses.
    pub fn get_cloned(&mut self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.get(key).cloned()
    }

    /// Inserts or replaces a value. Replacing an existing key keeps its FIFO
    /// position; a new key evicts the oldest resident key at capacity.
    pub fn insert(&mut self, key: K, value: V) {
        if self.capacity == 0 {
            return;
        }
        if let Some(existing) = self.values.get_mut(&key) {
            *existing = value;
            return;
        }
        while self.values.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                self.values.clear();
                break;
            };
            if self.values.remove(&oldest).is_some() {
                self.stats.evictions = self.stats.evictions.saturating_add(1);
                break;
            }
        }
        self.order.push_back(key.clone());
        self.values.insert(key, value);
    }

    /// Removes all entries while retaining cumulative hit/miss/eviction data.
    pub fn clear(&mut self) {
        self.values.clear();
        self.order.clear();
    }

    pub const fn stats(&self) -> GlyphCacheStats {
        self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_hits_and_misses() {
        let mut cache = GlyphCache::with_capacity(2);
        assert_eq!(cache.get(&1), None);
        cache.insert(1, 10);
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[test]
    fn fifo_evicts_oldest_without_promoting_hits() {
        let mut cache = GlyphCache::with_capacity(2);
        cache.insert(1, "one");
        cache.insert(2, "two");
        assert_eq!(cache.get(&1), Some(&"one"));
        cache.insert(3, "three");
        assert!(cache.get(&1).is_none());
        assert_eq!(cache.get(&2), Some(&"two"));
        assert_eq!(cache.get(&3), Some(&"three"));
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn generation_isolation_prevents_cross_font_reuse() {
        let mut cache = GlyphCache::with_capacity(4);
        let old = GlyphCacheKey::new('A', 14, 7);
        let new_generation = GlyphCacheKey::new('A', 14, 8);
        cache.insert(old, 1);
        assert!(cache.get(&new_generation).is_none());
        assert_eq!(cache.get(&old), Some(&1));
    }

    #[test]
    fn size_and_character_are_isolated() {
        let mut cache = GlyphCache::with_capacity(4);
        let base = GlyphCacheKey::new('A', 14, 1);
        cache.insert(base, 1);
        assert!(cache.get(&GlyphCacheKey::new('A', 15, 1)).is_none());
        assert!(cache.get(&GlyphCacheKey::new('B', 14, 1)).is_none());
        assert_eq!(cache.get(&base), Some(&1));
    }
}
