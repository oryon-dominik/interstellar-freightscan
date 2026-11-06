//! Files that carry other languages: Markdown fences, notebook cells, script and style in HTML.
//!
//! Every function returns segments: a language and the kinds of its consecutive lines. Embedded
//! text counts under its own language; the container's own lines count under the container.

use crate::languages::{self, Language};
use crate::types::Kind;
use crate::{python, scanner};

pub type Segment = (&'static Language, Vec<Kind>);

/// Classify every line of a file in `language`, following the languages it embeds.
pub fn segments(text: &str, language: &'static Language) -> Vec<Segment> {
    if *language == languages::MARKDOWN {
        markdown(text, &languages::MARKDOWN)
    } else if *language == languages::NOTEBOOK {
        notebook(text)
    } else if *language == languages::HTML || *language == languages::DJANGO_TEMPLATE {
        html(text, language)
    } else {
        vec![(language, single(text, language))]
    }
}

/// Classify text in one language — Python exactly when it tokenizes, by comment markers otherwise.
pub fn single(text: &str, language: &Language) -> Vec<Kind> {
    if *language == languages::PYTHON
        && let Some(kinds) = python::kinds(text)
    {
        return kinds;
    }
    scanner::kinds(text, language)
}

/// The opening of a fence: its marker run and the language tag behind it.
fn fence(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start();
    let marker = rest.as_bytes().first().filter(|byte| matches!(byte, b'`' | b'~'))?;
    let length = rest.bytes().take_while(|byte| byte == marker).count();
    if length < 3 {
        return None;
    }
    let (run, after) = rest.split_at(length);
    let after = after.trim_start();
    let end =
        after.find(|character: char| !(character.is_alphanumeric() || matches!(character, '_' | '+' | '-' | '.')));
    Some((run, &after[..end.unwrap_or(after.len())]))
}

/// Prose and fence lines count under `container`, fenced code under the language it names.
pub fn markdown(text: &str, container: &'static Language) -> Vec<Segment> {
    let mut result = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut open: Option<&str> = None;
    let mut code: Vec<&str> = Vec::new();
    let mut embedded: &'static Language = &languages::UNKNOWN;
    for line in scanner::split(text) {
        match open {
            None => match fence(line) {
                Some((run, tag)) => {
                    result.extend(prose_segment(&prose, container));
                    prose.clear();
                    open = Some(run);
                    embedded = languages::by_tag(&tag.to_ascii_lowercase());
                    result.push((container, vec![Kind::Prose]));
                }
                None => prose.push(line),
            },
            Some(run) if line.trim_start().starts_with(run) => {
                result.extend(fenced(&code, embedded, container));
                code.clear();
                open = None;
                result.push((container, vec![Kind::Prose]));
            }
            Some(_) => code.push(line),
        }
    }
    if open.is_some() {
        result.extend(fenced(&code, embedded, container));
    } else {
        result.extend(prose_segment(&prose, container));
    }
    result
}

/// Code cells count under the kernel's language, markdown cells as the notebook's prose.
pub fn notebook(text: &str) -> Vec<Segment> {
    let unread = || vec![(&languages::NOTEBOOK, scanner::kinds(text, &languages::UNKNOWN))];
    let Ok(document) = serde_json::from_str::<serde_json::Value>(text) else { return unread() };
    let Some(cells) = document.get("cells").and_then(|cells| cells.as_array()) else { return unread() };
    let metadata = &document["metadata"];
    let tag = metadata["kernelspec"]["language"]
        .as_str()
        .or_else(|| metadata["language_info"]["name"].as_str())
        .unwrap_or_default();
    let kernel = languages::by_tag(&tag.to_ascii_lowercase());
    let mut result = Vec::new();
    for cell in cells {
        let source = match &cell["source"] {
            serde_json::Value::Array(parts) => parts.iter().filter_map(|part| part.as_str()).collect::<String>(),
            serde_json::Value::String(text) => text.clone(),
            _ => String::new(),
        };
        match cell["cell_type"].as_str() {
            Some("code") => result.push((kernel, single(&source, kernel))),
            Some("markdown") => result.extend(markdown(&source, &languages::NOTEBOOK)),
            _ => result.push((&languages::NOTEBOOK, scanner::kinds(&source, &languages::UNKNOWN))),
        }
    }
    result
}

/// The element a line opens with `<script …>` or `<style …>`, when its closing tag is not on the same line.
fn opened_element(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    ["script", "style"].into_iter().find(|element| {
        let opening = format!("<{element}");
        lower.match_indices(&opening).any(|(at, _)| {
            let rest = &lower[at + opening.len()..];
            let bounded =
                rest.chars().next().is_none_or(|character| !(character.is_alphanumeric() || character == '_'));
            bounded && rest.contains('>') && !rest.contains(&format!("</{element}"))
        })
    })
}

/// Markup counts under `language`, the content of script and style elements under JavaScript and CSS.
pub fn html(text: &str, language: &'static Language) -> Vec<Segment> {
    let embedded = |element: &str| if element == "script" { &languages::JAVASCRIPT } else { &languages::CSS };
    let mut result: Vec<Segment> = Vec::new();
    let mut markup: Vec<&str> = Vec::new();
    let mut content: Vec<&str> = Vec::new();
    let mut element: Option<&str> = None;
    for line in scanner::split(text) {
        match element {
            None => {
                markup.push(line);
                if let Some(opened) = opened_element(line) {
                    element = Some(opened);
                    result.push((language, scanner::kinds_of_lines(&markup, language)));
                    markup.clear();
                }
            }
            Some(open) if line.to_ascii_lowercase().contains(&format!("</{open}")) => {
                result.push((embedded(open), scanner::kinds_of_lines(&content, embedded(open))));
                content.clear();
                element = None;
                markup.push(line);
            }
            Some(_) => content.push(line),
        }
    }
    if let Some(open) = element {
        result.push((embedded(open), scanner::kinds_of_lines(&content, embedded(open))));
    }
    if !markup.is_empty() {
        result.push((language, scanner::kinds_of_lines(&markup, language)));
    }
    result
}

/// Markdown text: blank, an HTML comment, or prose.
fn prose_segment(block: &[&str], container: &'static Language) -> Option<Segment> {
    if block.is_empty() {
        return None;
    }
    let kinds = scanner::kinds_of_lines(block, &languages::MARKDOWN);
    Some((container, kinds.into_iter().map(|kind| if kind == Kind::Code { Kind::Prose } else { kind }).collect()))
}

/// Fenced lines: classified in a known language, unclassified Markdown otherwise.
fn fenced(code: &[&str], language: &'static Language, container: &'static Language) -> Option<Segment> {
    if code.is_empty() {
        None
    } else if *language == languages::UNKNOWN {
        Some((container, scanner::kinds_of_lines(code, &languages::UNKNOWN)))
    } else if *language == languages::MARKDOWN {
        prose_segment(code, container)
    } else {
        let text: String = code.iter().map(|line| format!("{line}\n")).collect();
        Some((language, single(&text, language)))
    }
}
