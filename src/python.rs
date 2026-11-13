//! Classify Python lines exactly, from one pass over a token stream.
//!
//! A docstring is a string that forms the whole first statement of a module, class or function.
//! The token stream shows that as well as a syntax tree does. The lexer here knows just enough
//! Python to find tokens and their lines: strings with every prefix, f-strings with nested
//! replacement fields, comments, brackets and line continuations. Indentation is not checked.

use crate::scanner;
use crate::types::Kind;

/// How deep replacement fields may nest; Python refuses f-strings nested this deep as well.
const MAX_FIELDS: usize = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    Name,
    Number,
    Op,
    /// A plain or byte string — the only kind that can be a docstring.
    String,
    /// An f-string or t-string: code, never a docstring.
    Template,
    Comment,
    /// The end of a logical line.
    Newline,
}

#[derive(Clone, Copy, Debug)]
struct Lexeme<'text> {
    token: Token,
    text: &'text str,
    start: usize,
    end: usize,
}

/// The text cannot be tokenized: an unterminated string or bracket, or a character Python rejects.
#[derive(Debug)]
struct Untokenizable;

/// Return the kind of every line, or None when the text cannot be tokenized.
pub fn kinds(text: &str) -> Option<Vec<Kind>> {
    let lexemes = Lexer::new(text).run().ok()?;
    let count = scanner::split(text).len();
    let mut code = vec![false; count + 2];
    let mut comments = vec![false; count + 2];
    for lexeme in &lexemes {
        match lexeme.token {
            Token::Comment => comments[lexeme.start] = true,
            Token::Newline => {}
            _ => code[lexeme.start..=lexeme.end].fill(true),
        }
    }
    let docstrings = docstring_lines(&lexemes, count);
    Some(
        (1..=count)
            .map(|number| {
                if docstrings[number] {
                    Kind::Docstring
                } else if code[number] {
                    Kind::Code
                } else if comments[number] {
                    Kind::Comment
                } else {
                    Kind::Blank
                }
            })
            .collect(),
    )
}

/// Mark the lines of every string that is the whole first statement of a body.
///
/// `expect` holds while the next statement may be a docstring: at the top of the module and
/// at the start of a `def` or `class` body. A body starts after the header's closing colon,
/// on the same line or after the line break.
fn docstring_lines(lexemes: &[Lexeme], count: usize) -> Vec<bool> {
    let mut lines = vec![false; count + 2];
    let mut expect = true;
    let mut statement_start = true;
    let mut header = false;
    let mut after_colon = false;
    let mut depth = 0i32;
    let mut pending: Option<(usize, usize)> = None;
    let mut previous_name = "";
    for lexeme in lexemes {
        let Lexeme { token, text, start, end } = *lexeme;
        if token == Token::Comment {
            continue;
        }
        if token == Token::Newline || (token == Token::Op && text == ";" && depth == 0) {
            if let Some((first, last)) = pending {
                lines[first..=last].fill(true);
            }
            expect = header && after_colon && token == Token::Newline;
            statement_start = true;
            header = false;
            after_colon = false;
            pending = None;
            continue;
        }
        if statement_start {
            // A body written behind the colon, on the header's own line, ends the header there.
            statement_start = false;
            after_colon = false;
            if token == Token::String && expect {
                pending = Some((start, end));
                continue;
            }
            expect = false;
            header = token == Token::Name && matches!(text, "def" | "class");
        } else if let (Some((first, _)), Token::String) = (pending, token) {
            pending = Some((first, end));
            continue;
        }
        pending = None;
        if token == Token::Name && previous_name == "async" && text == "def" {
            header = true;
        }
        previous_name = if token == Token::Name { text } else { "" };
        if token == Token::Op {
            match text {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth -= 1,
                ":" if header && depth == 0 => {
                    after_colon = true;
                    statement_start = true;
                    expect = true;
                }
                _ => {}
            }
        }
    }
    lines
}

struct Lexer<'text> {
    text: &'text str,
    bytes: &'text [u8],
    position: usize,
    line: usize,
    depth: usize,
    fields: usize,
    line_has_tokens: bool,
    lexemes: Vec<Lexeme<'text>>,
}

impl<'text> Lexer<'text> {
    fn new(text: &'text str) -> Self {
        Self {
            text,
            bytes: text.as_bytes(),
            position: 0,
            line: 1,
            depth: 0,
            fields: 0,
            line_has_tokens: false,
            lexemes: Vec::new(),
        }
    }

    fn peek(&self, offset: usize) -> u8 {
        self.bytes.get(self.position + offset).copied().unwrap_or(0)
    }

    fn push(&mut self, token: Token, from: usize, start: usize) {
        self.lexemes.push(Lexeme { token, text: &self.text[from..self.position], start, end: self.line });
        if token != Token::Comment {
            self.line_has_tokens = true;
        }
    }

    fn run(mut self) -> Result<Vec<Lexeme<'text>>, Untokenizable> {
        while self.position < self.bytes.len() {
            let byte = self.peek(0);
            let from = self.position;
            let start = self.line;
            match byte {
                b'\n' => {
                    self.end_line();
                    self.position += 1;
                    self.line += 1;
                }
                b' ' | b'\t' | b'\r' | b'\x0c' => self.position += 1,
                b'\\' => {
                    // A line continuation: the logical line goes on.
                    let skip = if self.peek(1) == b'\r' { 2 } else { 1 };
                    if self.peek(skip) != b'\n' {
                        return Err(Untokenizable);
                    }
                    self.position += skip + 1;
                    self.line += 1;
                }
                b'#' => {
                    while self.position < self.bytes.len() && self.peek(0) != b'\n' {
                        self.position += 1;
                    }
                    self.push(Token::Comment, from, start);
                }
                b'\'' | b'"' => {
                    self.string(false)?;
                    self.push(Token::String, from, start);
                }
                b'0'..=b'9' => self.number(from, start),
                b'.' if self.peek(1).is_ascii_digit() => self.number(from, start),
                _ if is_identifier_start(byte) => self.name_or_string(from, start)?,
                b'(' | b'[' | b'{' => {
                    self.depth += 1;
                    self.position += 1;
                    self.push(Token::Op, from, start);
                }
                b')' | b']' | b'}' => {
                    self.depth = self.depth.checked_sub(1).ok_or(Untokenizable)?;
                    self.position += 1;
                    self.push(Token::Op, from, start);
                }
                b'$' | b'?' | b'`' => return Err(Untokenizable),
                _ => {
                    // `:=` is no header colon; every other operator is told apart by its first byte alone.
                    self.position += if byte == b':' && self.peek(1) == b'=' { 2 } else { 1 };
                    self.push(Token::Op, from, start);
                }
            }
        }
        if self.depth > 0 {
            return Err(Untokenizable);
        }
        self.end_line();
        Ok(self.lexemes)
    }

    fn end_line(&mut self) {
        if self.depth == 0 && self.line_has_tokens {
            let at = self.position;
            self.lexemes.push(Lexeme {
                token: Token::Newline,
                text: &self.text[at..at],
                start: self.line,
                end: self.line,
            });
            self.line_has_tokens = false;
        }
    }

    fn number(&mut self, from: usize, start: usize) {
        while let byte @ (b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'.' | b'+' | b'-') = self.peek(0) {
            if matches!(byte, b'+' | b'-') && !matches!(self.bytes[self.position - 1], b'e' | b'E') {
                break;
            }
            self.position += 1;
        }
        self.push(Token::Number, from, start);
    }

    fn name_or_string(&mut self, from: usize, start: usize) -> Result<(), Untokenizable> {
        while is_identifier_continue(self.peek(0)) {
            self.position += 1;
        }
        let prefix = self.text[from..self.position].to_ascii_lowercase();
        if matches!(self.peek(0), b'\'' | b'"') && is_string_prefix(&prefix) {
            let template = prefix.contains('f') || prefix.contains('t');
            self.string(template)?;
            self.push(if template { Token::Template } else { Token::String }, from, start);
        } else {
            self.push(Token::Name, from, start);
        }
        Ok(())
    }

    /// Consume a string literal from its opening quote; a template string follows its replacement fields.
    fn string(&mut self, template: bool) -> Result<(), Untokenizable> {
        let quote = self.peek(0);
        let triple = self.peek(1) == quote && self.peek(2) == quote;
        self.position += if triple { 3 } else { 1 };
        loop {
            match self.peek(0) {
                0 if self.position >= self.bytes.len() => return Err(Untokenizable),
                b'\\' => {
                    // Raw or not, a backslash keeps the next character from closing the string.
                    let skip = if self.peek(1) == b'\r' && self.peek(2) == b'\n' { 2 } else { 1 };
                    if self.peek(skip) == b'\n' {
                        self.line += 1;
                    }
                    self.position += skip + 1;
                }
                b'\n' if !triple => return Err(Untokenizable),
                b'\n' => {
                    self.line += 1;
                    self.position += 1;
                }
                byte if byte == quote => {
                    if !triple {
                        self.position += 1;
                        return Ok(());
                    }
                    if self.peek(1) == quote && self.peek(2) == quote {
                        self.position += 3;
                        return Ok(());
                    }
                    self.position += 1;
                }
                b'{' if template && self.peek(1) == b'{' => self.position += 2,
                b'{' if template => {
                    self.position += 1;
                    self.replacement_field()?;
                }
                _ => self.position += 1,
            }
        }
    }

    /// Consume a replacement field after its `{`, refusing a nesting that would exhaust the stack.
    fn replacement_field(&mut self) -> Result<(), Untokenizable> {
        if self.fields == MAX_FIELDS {
            return Err(Untokenizable);
        }
        self.fields += 1;
        let result = self.field_body();
        self.fields -= 1;
        result
    }

    /// An expression, then a format spec that may nest fields.
    fn field_body(&mut self) -> Result<(), Untokenizable> {
        let mut depth = 0usize;
        let mut in_spec = false;
        loop {
            let byte = self.peek(0);
            if byte == 0 && self.position >= self.bytes.len() {
                return Err(Untokenizable);
            }
            match byte {
                b'\n' => {
                    self.line += 1;
                    self.position += 1;
                }
                b'}' if depth == 0 => {
                    self.position += 1;
                    return Ok(());
                }
                b'{' if in_spec => {
                    self.position += 1;
                    self.replacement_field()?;
                }
                _ if in_spec => self.position += 1,
                b'(' | b'[' | b'{' => {
                    depth += 1;
                    self.position += 1;
                }
                b')' | b']' | b'}' => {
                    depth = depth.saturating_sub(1);
                    self.position += 1;
                }
                b':' if depth == 0 => {
                    in_spec = true;
                    self.position += 1;
                }
                b'\'' | b'"' => self.string(false)?,
                _ if is_identifier_start(byte) => {
                    let from = self.position;
                    while is_identifier_continue(self.peek(0)) {
                        self.position += 1;
                    }
                    let prefix = self.text[from..self.position].to_ascii_lowercase();
                    if matches!(self.peek(0), b'\'' | b'"') && is_string_prefix(&prefix) {
                        self.string(prefix.contains('f') || prefix.contains('t'))?;
                    }
                }
                _ => self.position += 1,
            }
        }
    }
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

fn is_string_prefix(prefix: &str) -> bool {
    matches!(prefix, "r" | "u" | "b" | "br" | "rb" | "f" | "fr" | "rf" | "t" | "tr" | "rt")
}

#[cfg(test)]
mod tests {
    use super::*;
    use Kind::*;

    #[test]
    fn shall_11_module_class_and_function_docstrings_are_docstrings() {
        let text = concat!(
            "\"\"\"Module.\"\"\"\n",
            "\n",
            "\n",
            "class Probe:\n",
            "    \"\"\"Class.\"\"\"\n",
            "\n",
            "    def launch(self):\n",
            "        \"\"\"Launch the probe.\n",
            "\n",
            "        Every line belongs to the docstring, the blank one too.\n",
            "        \"\"\"\n",
            "        return 1\n",
        );
        assert_eq!(
            kinds(text).unwrap(),
            [Docstring, Blank, Blank, Code, Docstring, Blank, Code, Docstring, Docstring, Docstring, Docstring, Code]
        );
    }

    #[test]
    fn shall_12_a_hash_inside_a_string_is_code() {
        assert_eq!(kinds("marker = \"# not a comment\"\n").unwrap(), [Code]);
    }

    #[test]
    fn a_string_that_is_not_the_first_statement_is_code() {
        assert_eq!(kinds("value = 1\n\"\"\"Not a docstring,\nonly a string.\"\"\"\n").unwrap(), [Code, Code, Code]);
    }

    #[test]
    fn a_comment_line_is_a_comment_and_a_trailing_comment_leaves_code() {
        assert_eq!(kinds("# why\nvalue = 1  # how\n").unwrap(), [Comment, Code]);
    }

    #[test]
    fn shall_23_a_file_that_does_not_parse_gives_no_classification() {
        assert!(kinds("def broken(:\n").is_none());
        assert!(kinds("value = 'open\n").is_none());
        assert!(kinds("value = \"\"\"open\n").is_none());
    }

    #[test]
    fn shall_11_docstrings_are_the_first_statement_of_module_class_and_function() {
        let cases: &[(&str, &str, &[Kind])] = &[
            ("after a shebang", "#!/usr/bin/env python\n\"\"\"Doc.\"\"\"\n", &[Comment, Docstring]),
            ("after an import", "from os import path\n\"\"\"Not first.\"\"\"\n", &[Code, Code]),
            ("one-line body", "def go(): \"Doc.\"\n", &[Docstring]),
            ("after a comment", "class Probe:\n    # why\n    \"\"\"Doc.\"\"\"\n", &[Code, Comment, Docstring]),
            ("decorated async", "@decorated\nasync def go():\n    \"\"\"Doc.\"\"\"\n", &[Code, Code, Docstring]),
            ("inside an async for", "async for item in items:\n    \"\"\"Not a docstring.\"\"\"\n", &[Code, Code]),
            (
                "colon inside the signature",
                "def go(x: dict = {1: 2}) -> int:\n    \"\"\"Doc.\"\"\"\n",
                &[Code, Docstring],
            ),
            ("signature over lines", "def go(\n    x,\n):\n    \"\"\"Doc.\"\"\"\n", &[Code, Code, Code, Docstring]),
            ("string with a call", "def go():\n    \"Doc.\".strip()\n", &[Code, Code]),
            ("f-string", "def go():\n    f\"\"\"Not a docstring.\"\"\"\n", &[Code, Code]),
            ("inside an if", "if True:\n    \"\"\"Not a docstring.\"\"\"\n", &[Code, Code]),
            ("implicit concatenation", "\"\"\"One\" \"two.\"\"\"\n", &[Docstring]),
            ("after a one-line body", "def go(): pass\n\"\"\"Not a docstring.\"\"\"\n", &[Code, Code]),
            ("after a one-line docstring", "def go(): \"Doc.\"\n\"\"\"Not a docstring.\"\"\"\n", &[Docstring, Code]),
            ("walrus in a default", "def go(x=(y := 1)):\n    \"\"\"Doc.\"\"\"\n", &[Code, Docstring]),
        ];
        for (name, text, expected) in cases {
            assert_eq!(kinds(text).unwrap(), *expected, "{name}");
        }
    }

    #[test]
    fn a_string_continued_by_backslashes_and_brackets_is_code_on_every_line() {
        assert_eq!(kinds("value = (\n    'one'\n    'two'\n)\n").unwrap(), [Code, Code, Code, Code]);
        assert_eq!(kinds("value = 1 + \\\n    2\n").unwrap(), [Code, Code]);
    }

    #[test]
    fn nested_quotes_and_braces_in_an_f_string_do_not_end_it() {
        let text = "value = f\"{'#' if x else {'a': 1}['a']:>{width}} }}\"\n# why\n";
        assert_eq!(kinds(text).unwrap(), [Code, Comment]);
    }

    #[test]
    fn a_triple_quoted_f_string_spans_its_lines() {
        assert_eq!(kinds("value = f\"\"\"\n{x}\n\"\"\"\n").unwrap(), [Code, Code, Code]);
    }

    #[test]
    fn a_raw_string_keeps_its_escaped_quote() {
        assert_eq!(kinds("pattern = r\"\\\"\"\n").unwrap(), [Code]);
    }

    #[test]
    fn a_backslash_continues_a_string_over_a_windows_line_end() {
        assert_eq!(kinds("value = \"one \\\r\ntwo\"\r\n").unwrap(), [Code, Code]);
    }

    #[test]
    fn replacement_fields_nested_past_python_s_limit_give_no_classification() {
        assert!(kinds(&format!("value = f\"{}\n", "{a:".repeat(1_000_000))).is_none());
        assert!(kinds(&format!("value = f\"{}\n", "{f\"".repeat(1_000_000))).is_none());
    }

    #[test]
    fn a_last_line_without_newline_still_ends_its_statement() {
        assert_eq!(kinds("\"\"\"Doc.\"\"\"").unwrap(), [Docstring]);
    }
}
