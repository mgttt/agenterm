use std::collections::HashMap;

use crate::{DynError, MAX_SYMBOLS};

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

    pub fn intern(&mut self, name: &str) -> Result<Symbol, DynError> {
        if let Some(&sym) = self.index.get(name) {
            return Ok(sym);
        }
        crate::Dyn::ensure_name(name)?;
        if self.strings.len() == MAX_SYMBOLS {
            return Err(DynError::StateLimit {
                resource: "symbols",
                limit: MAX_SYMBOLS,
            });
        }
        let id = u32::try_from(self.strings.len()).expect("symbol table limit fits u32");
        let sym = Symbol::from_raw(id);
        self.strings.push(name.to_owned());
        self.index.insert(name.to_owned(), sym);
        Ok(sym)
    }
}
