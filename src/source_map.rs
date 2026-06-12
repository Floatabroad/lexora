use crate::span::Span;

pub struct SourceFile<'a> {
    pub name: String,
    pub base: u32,
    pub src: &'a str,
}

pub struct Resolved<'a> {
    pub name: &'a str,
    pub line: usize,
    pub col: usize,
    pub line_text: &'a str,
    pub caret_col: usize,
    pub caret_len: usize,
}

pub struct SourceMap<'a> {
    files: Vec<SourceFile<'a>>,
    next_base: u32,
}

impl<'a> SourceMap<'a> {
    pub fn new() -> Self {
        SourceMap { files: Vec::new(), next_base: 0 }
    }
    pub fn add(&mut self, name: String, src:&'a str) -> u32{
        let base = self.next_base;
        self.next_base = base + src.len() as u32 +1;
        self.files.push(SourceFile { name, base, src });
        base
    }
    fn file_of(&self, offset: u32) -> Option<&SourceFile<'a>> {
        self.files.iter().rev().find(|f| offset >= f.base)
    }
    pub fn resolve(&self, span: Span) -> Option<Resolved<'_>> {
        let file = self.file_of(span.start)?;
        let src = file.src;

        let start = ((span.start - file.base) as usize).min(src.len());
        let end = ((span.end - file.base) as usize).min(src.len()).max(start);

        let line_start = src[..start].rfind('\n').map(|i| i+1).unwrap_or(0);
        let line_end = src[start..].find('\n').map(|i| i+start).unwrap_or(src.len());
        let line_text = &src[line_start..line_end];

        let line = src[..line_start].bytes().filter(|&b| b == b'\n').count() + 1;
        let caret_col = src[line_start..start].chars().count();
        let col = caret_col + 1;

        let len_bytes = (end - start).min(line_end - start);
        let caret_len = src[start..start + len_bytes].chars().count().max(1);

        Some(Resolved { name: &file.name, line, col, line_text, caret_col, caret_len})
    }
}