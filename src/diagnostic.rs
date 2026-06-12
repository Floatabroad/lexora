use crate::error::LexoraError;
use crate::source_map::SourceMap;
use crate::span::Span;


#[derive(Clone, Copy, PartialEq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NoteKind{
    Note,
    Help,
}

pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub primary: Option<Span>,
    pub label: Option<String>,
    pub notes: Vec<(NoteKind, String)>,
}


pub fn to_diagnostic(err: &LexoraError) -> Diagnostic {
    let mut notes = Vec::new();
    let (message, label) = match err {
        LexoraError::UnexpectedToken {expected, found, ..} => (
            format!("beklenmeyen ifade: {}", found),
            Some(format!("beklenen: {}", expected)),
            ),
        LexoraError::UndefinedVariable {name, ..} => (
            format!("tanimsiz degisken: {}", name),
            Some("bu kapsamda tanimli degil".to_string()),
            ),
        LexoraError::UndefinedFunction {name, ..} => (
            format!("tanimsiz fonksiyon: {}", name),
            Some("bu isimde bir fonksiyon yok".to_string()),
            ),
        LexoraError::TypeMismatch {expected, found, ..} => {
            notes.push((NoteKind::Help, format!("beklenen tip: `{}`", expected)));
            (
                format!("tip uyusmazligi: beklenen {}, bulunan {}", expected, found),
                Some(format!("bu ifade `{}` tipinde", found)),
                )
        }
        LexoraError::AlreadyDefined {name, ..} => (
            format!("zaten tanimli: {}", name),
            Some("yeniden tanimlandi".to_string()),
        ),
        LexoraError::InvalidCast {from, to, ..} => {
            notes.push((NoteKind::Note, "gecerli cast'ler: i32 -> i64, i64 -> i32".to_string()));
            (
                format!("gecersiz cast: {} -> {}", from, to),
                Some(format!("`{}` -> `{}` desteklenmiyor", from, to)),
            )
        }
        LexoraError::Custom { message, .. } => (message.clone(), None),
        LexoraError::Codegen { message } => (
            format!("ic derleyici hatasi (codegen): {}", message),
            None,
        ),
    };
    Diagnostic {
        level: Level::Error,
        message,
        primary: err.span(),
        label,
        notes,
    }
}
pub fn render(sources: &SourceMap, diag: &Diagnostic, color: bool) -> String {
    let (red, yellow, bold, blue, reset) = if color {
        ("\x1b[31m", "\x1b[33m", "\x1b[1m", "\x1b[34m", "\x1b[0m")
    } else {
        ("", "", "", "", "")
    };
    let (lvl_color, lvl_word) = match diag.level {
        Level::Error => (red, "error"),
        Level::Warning => (yellow, "warning"),
    };

    let mut out = String::new();
    out.push_str(&format!(
        "{bold}{lvl_color}{lvl_word}{reset}{bold}: {}{reset}\n",
        diag.message
    ));

    let loc = match diag.primary.and_then(|s| sources.resolve(s)) {
        Some(l) => l,
        None => return out,
    };

    let mut pad = String::new();
    for c in loc.line_text.chars().take(loc.caret_col) {
        pad.push_str(if c == '\t' { "    " } else { " " });
    }
    let line_text = loc.line_text.replace('\t', "    ");
    let carets = "^".repeat(loc.caret_len);
    let num = loc.line.to_string();
    let g = " ".repeat(num.len());

    out.push_str(&format!("{blue}{g}--> {reset}{}:{}:{}\n", loc.name, loc.line,
                          loc.col));
    out.push_str(&format!("{blue}{g} |{reset}\n"));
    out.push_str(&format!("{blue}{num} |{reset} {line_text}\n"));

    let label = match &diag.label {
        Some(l) => format!(" {}", l),
        None => String::new(),
    };
    out.push_str(&format!(
        "{blue}{g} |{reset} {pad}{lvl_color}{carets}{label}{reset}\n"
    ));
    if !diag.notes.is_empty() {
        out.push_str(&format!("{blue}{g} |{reset}\n"));
    }
    for (kind, text) in &diag.notes {
        let word = match kind {
            NoteKind::Note => "note",
            NoteKind::Help => "help",
        };
        out.push_str(&format!("{blue}{g} = {bold}{word}{reset}: {}\n", text));
    }

    out
}