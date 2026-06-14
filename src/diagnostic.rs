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
        LexoraError::UndefinedVariable {name, suggestion, ..} => {
            if let Some(s) = suggestion {
                notes.push((NoteKind::Help, format!("bunu mu demek istediniz: `{}`?",
                                                    s)));
            }
            (
                format!("tanimsiz degisken: {}", name),
                Some("bu kapsamda tanimli degil".to_string()),
            )
        }
        LexoraError::UndefinedFunction {name, suggestion, ..} => {
            if let Some(s) = suggestion {
                notes.push((NoteKind::Help, format!("bunu mu demek istediniz: `{}`?",
                                                    s)));
            }
            (
                format!("tanimsiz fonksiyon: {}", name),
                Some("bu isimde bir fonksiyon yok".to_string()),
            )
        }
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
fn disp_width(text: &str, char_count: usize) -> usize {
    text.chars().take(char_count).map(|c| if c == '\t' { 4 } else { 1 }).sum()
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

    let g = " ".repeat(loc.lines.last().unwrap().0.to_string().len());

    out.push_str(&format!("{blue}{g}--> {reset}{}:{}:{}\n", loc.name, loc.line,
                          loc.col));
    out.push_str(&format!("{blue}{g} |{reset}\n"));

    if loc.lines.len() == 1 {
        let (num, text) = loc.lines[0];
        let line_text = text.replace('\t', "    ");
        let pad = " ".repeat(disp_width(text, loc.start_char));
        let carets = "^".repeat((loc.end_char - loc.start_char).max(1));
        let label = match &diag.label {
            Some(l) => format!(" {}", l),
            None => String::new(),
        };
        out.push_str(&format!("{blue}{num} |{reset} {line_text}\n"));
        out.push_str(&format!(
            "{blue}{g} |{reset} {pad}{lvl_color}{carets}{label}{reset}\n"
        ));
    } else {
        let last = loc.lines.len() - 1;
        let start_col = 3 + disp_width(loc.lines[0].1, loc.start_char);
        for (i, (num, text)) in loc.lines.iter().enumerate() {
            let num_s = num.to_string();
            let lpad = " ".repeat(g.len() - num_s.len());
            let line_text = text.replace('\t', "    ");
            if i == 0 {
                out.push_str(&format!("{blue}{num_s}{lpad} |{reset}   {line_text}\n"));
                let unders = "_".repeat(start_col - 1);
                out.push_str(&format!("{blue}{g} |{reset} {lvl_color}{unders}^{reset}\n"));
            } else {
                out.push_str(&format!(
                    "{blue}{num_s}{lpad} |{reset} {lvl_color}|{reset} {line_text}\n"
                ));
            }
        }
        let end_col = 3 + disp_width(loc.lines[last].1, loc.end_char).saturating_sub(1);
        let unders = "_".repeat(end_col.saturating_sub(2));
        let label = match &diag.label {
            Some(l) => format!(" {}", l),
            None => String::new(),
        };
        out.push_str(&format!(
            "{blue}{g} |{reset} {lvl_color}|{unders}^{label}{reset}\n"
        ));
    }

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
pub fn render_summary(count: usize, color: bool) -> String {
    let (red, bold, reset) = if color {
        ("\x1b[31m", "\x1b[1m", "\x1b[0m")
    } else {
        ("", "", "")
    };
    format!("{bold}{red}error{reset}{bold}: {} hata yuzunden durduruldu{reset}\n", count)
}