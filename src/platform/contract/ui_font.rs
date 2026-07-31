//! OS-neutral font-file candidate descriptors for native frontend rendering.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FontFileCandidate {
    pub(crate) name: &'static str,
    pub(crate) components: &'static [&'static str],
}
