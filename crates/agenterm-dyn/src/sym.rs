use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(u32);

impl Symbol {
    pub(crate) fn from_raw(id: u32) -> Self {
        Self(id)
    }

    pub fn index(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Default)]
pub(crate) struct Interner {
    strings: Vec<String>,
    index: HashMap<String, Symbol>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, name: &str) -> Symbol {
        if let Some(&sym) = self.index.get(name) {
            return sym;
        }
        let id = u32::try_from(self.strings.len()).expect("symbol table overflow");
        let sym = Symbol::from_raw(id);
        self.strings.push(name.to_owned());
        self.index.insert(name.to_owned(), sym);
        sym
    }
}
