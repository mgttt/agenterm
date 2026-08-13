//! Compact stable-tab-id storage for the lightweight console host.
//!
//! Tree order and parentage belong to `workspace`; this module only maps an
//! id to an owned value. Linear lookup avoids general-purpose map machinery
//! for con's deliberately small interactive tab sets. Storage order is not a
//! public contract, so removal may use `swap_remove`.

use crate::workspace::TabId;

pub(super) struct SessionStore<T> {
    entries: Vec<(TabId, T)>,
}

impl<T> Default for SessionStore<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T> SessionStore<T> {
    pub(super) fn insert(&mut self, id: TabId, session: T) -> Result<(), T> {
        if self.contains_key(&id) {
            return Err(session);
        }
        self.entries.push((id, session));
        Ok(())
    }

    pub(super) fn get(&self, id: &TabId) -> Option<&T> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, session)| session)
    }

    pub(super) fn get_mut(&mut self, id: &TabId) -> Option<&mut T> {
        self.entries
            .iter_mut()
            .find(|(candidate, _)| candidate == id)
            .map(|(_, session)| session)
    }

    pub(super) fn remove(&mut self, id: &TabId) -> Option<T> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _)| candidate == id)?;
        Some(self.entries.swap_remove(index).1)
    }

    pub(super) fn contains_key(&self, id: &TabId) -> bool {
        self.entries.iter().any(|(candidate, _)| candidate == id)
    }

    pub(super) fn entries_mut(&mut self) -> &mut [(TabId, T)] {
        self.entries.as_mut_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_own_values_independently_of_storage_order() {
        let first = TabId::new(1);
        let middle = TabId::new(2);
        let last = TabId::new(3);
        let mut store = SessionStore::default();
        store.insert(first, "first".to_owned()).unwrap();
        store.insert(middle, "middle".to_owned()).unwrap();
        store.insert(last, "last".to_owned()).unwrap();

        store.get_mut(&last).unwrap().push('!');
        assert_eq!(store.remove(&middle).as_deref(), Some("middle"));
        assert_eq!(store.get(&first).map(String::as_str), Some("first"));
        assert_eq!(store.get(&last).map(String::as_str), Some("last!"));
        assert!(!store.contains_key(&middle));
    }

    #[test]
    fn duplicate_id_is_rejected_without_replacing_the_live_session() {
        let id = TabId::new(1);
        let mut store = SessionStore::default();
        store.insert(id, "original").unwrap();

        assert_eq!(store.insert(id, "duplicate"), Err("duplicate"));
        assert_eq!(store.get(&id), Some(&"original"));
    }
}
