//! OS-neutral font-file candidate descriptors for native frontend rendering.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FontFileCandidate {
    pub name: &'static str,
    pub components: &'static [&'static str],
}
