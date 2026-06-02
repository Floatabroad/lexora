use crate::symbol::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Const(i64),
    Temp(u32),
    Global(Symbol),
    Named(String),
    Void,
}

impl Value {
    pub fn to_ir_str(&self) -> String {
        match self {
            Value::Const(n) => n.to_string(),
            Value::Temp(id) => format!("%t{}", id),
            Value::Global(sym) => format!("@g{}", sym.0),
            Value::Named(s) =>s.clone(),
            Value::Void => "void".to_string(),
        }
    }
}