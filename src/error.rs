use std::fmt;
use crate::span::Span;


#[derive(Debug)]
pub enum LexoraError {
    UnexpectedToken {
        expected: String,
        found: String,
        span: Span,
    },
    UndefinedVariable {
        name: String,
        span: Span,
    },
    UndefinedFunction {
        name: String,
        span: Span,
    },
    TypeMismatch {
        expected: String,
        found: String,
        span: Span,
    },
    AlreadyDefined {
        name: String,
        span: Span,
    },
    InvalidCast {
        from: String,
        to: String,
        span: Span,
    },
    Custom {
        message: String,
        span: Span,
    },
    Codegen {
        message: String,
    }
}

impl LexoraError {
    pub fn span(&self) -> Option<Span> {
        match self {
            LexoraError::UnexpectedToken {span, ..}
                | LexoraError::UndefinedVariable {span, ..}
                | LexoraError::UndefinedFunction {span, ..}
                | LexoraError::TypeMismatch {span, ..}
                | LexoraError::AlreadyDefined {span, ..}
                | LexoraError::InvalidCast {span, ..}
                | LexoraError::Custom {span, ..} => Some(*span),
            LexoraError::Codegen { .. } => None
        }
    }
}

impl fmt::Display for LexoraError{
    fn fmt(&self, f: &mut fmt::Formatter<'_> ) -> fmt::Result {
        match self {
            LexoraError::UnexpectedToken { expected, found, .. } =>
                write!(f, "beklenen: {}, bulunan: {}", expected, found),
            LexoraError::UndefinedVariable {name, .. } =>
                write!(f, "tanimsiz degisken: {}", name),
            LexoraError::UndefinedFunction {name, .. } =>
                write!(f, "tanimsiz fonksiyon: {}", name),
            LexoraError::TypeMismatch {expected, found, .. } =>
                write!(f, "Tip uyusmazlıgı: beklenen {}, bulunan {}", expected, found),
            LexoraError::AlreadyDefined {name, .. } =>
                write!(f, "Zaten tanımli '{}'", name),
            LexoraError::InvalidCast {from, to, .. } =>
                write!(f, "Gecersiz cast: {} -> {}", from, to),
            LexoraError::Custom { message, .. } =>
                write!(f, "{}", message),
            LexoraError::Codegen { message} =>
                write!(f, "ic derleyici hatasi (codegen): {}", message),
        }
    }
}

impl std::error::Error for LexoraError {}
                
