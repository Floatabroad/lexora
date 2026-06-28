use crate::symbol::Symbol;
#[derive(Debug, Clone, PartialEq)]
pub enum LlvmType {
    I1,
    I32,
    I64,
    Ptr,
    Void,
    Array(Box<LlvmType>, usize),
    Struct(Symbol),
    Enum(Symbol),
}


impl LlvmType {
    pub fn to_ir_str(&self) -> String {
        match self {
            LlvmType::I1 => "i1".to_string(),
            LlvmType::I32 => "i32".to_string(),
            LlvmType::I64 => "i64".to_string(),
            LlvmType::Ptr => "ptr".to_string(),
            LlvmType::Void => "void".to_string(),
            LlvmType::Array(t, n) => format!("[{} x {}]", n, t.to_ir_str()),
            LlvmType::Struct(s) => format!("%struct.{}", s.0),
            LlvmType::Enum(e) => format!("%enum.{}", e.0),
        }
    }
}