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
    pub lines: Vec<(usize, &'a str)>,
    pub start_char: usize,
    pub end_char: usize,
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
        let mut end = ((span.end - file.base) as usize).min(src.len()).max(start);

        let l1_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line = src[..l1_start].bytes().filter(|&b| b == b'\n').count() + 1;
        let start_char = src[l1_start..start].chars().count();
        let col = start_char + 1;

        if end > start && src.as_bytes()[end - 1] == b'\n' {
            end -= 1;
        }
        let l2_start = src[..end].rfind('\n').map(|i| i + 1).unwrap_or(0).max(l1_start);
        let end_char = src[l2_start..end].chars().count();

        let mut lines = Vec::new();
        let mut ls = l1_start;
        let mut n = line;


        loop {
            let le = src[ls..].find('\n').map(|i| i + ls).unwrap_or(src.len());
            lines.push((n, &src[ls..le]));
            if ls >= l2_start{
                break;
            }
            ls = le + 1;
            n += 1;
        }
        Some(Resolved{ name: &file.name, line, col, lines, start_char, end_char})
    }
    pub fn entry_name(&self) -> &str {
        &self.files[0].name
    }
}