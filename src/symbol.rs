use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Symbol(pub u32);

pub struct Interner {
    map: HashMap<String, Symbol>,
    vec: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Interner {
            map: HashMap::new(),
            vec: Vec::new(),
        }
    }
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s){
            return sym;
        }
        let sym = Symbol(self.vec.len() as u32);
        self.vec.push(s.to_string());
        self.map.insert(s.to_string(), sym);
        sym
    }
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.vec[sym.0 as usize]
    }
}