//! The languages freightscan knows: their names, their comment syntax and how a path finds them.

/// Comment syntax of one language — enough for the line scanner to tell comment from code.
///
/// `doc_lines` and `doc_blocks` open documentation instead of a plain comment; a block whose
/// lines name one of `help_keywords` after a dot is documentation too (PowerShell's help).
/// A `prose` language holds text, not code: its lines count as prose.
#[derive(Debug)]
pub struct Language {
    pub name: &'static str,
    pub line_comments: &'static [&'static str],
    pub blocks: &'static [(&'static str, &'static str)],
    pub nested: bool,
    pub doc_lines: &'static [&'static str],
    pub doc_blocks: &'static [&'static str],
    pub help_keywords: &'static [&'static str],
    pub prose: bool,
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Language {}

const fn plain(name: &'static str, line_comments: &'static [&'static str]) -> Language {
    Language {
        name,
        line_comments,
        blocks: &[],
        nested: false,
        doc_lines: &[],
        doc_blocks: &[],
        help_keywords: &[],
        prose: false,
    }
}

const fn blocks(
    name: &'static str,
    line_comments: &'static [&'static str],
    blocks: &'static [(&'static str, &'static str)],
) -> Language {
    Language { blocks, ..plain(name, line_comments) }
}

const C_BLOCK: &[(&str, &str)] = &[("/*", "*/")];
const XML_BLOCK: &[(&str, &str)] = &[("<!--", "-->")];

const POWERSHELL_HELP: &[&str] = &[
    "SYNOPSIS",
    "DESCRIPTION",
    "PARAMETER",
    "EXAMPLE",
    "INPUTS",
    "OUTPUTS",
    "NOTES",
    "LINK",
    "COMPONENT",
    "ROLE",
    "FUNCTIONALITY",
    "FORWARDHELPTARGETNAME",
    "FORWARDHELPCATEGORY",
    "REMOTEHELPRUNSPACE",
    "EXTERNALHELP",
];

pub static PYTHON: Language = plain("Python", &["#"]);
pub static TOML: Language = plain("TOML", &["#"]);
pub static YAML: Language = plain("YAML", &["#"]);
pub static JUSTFILE: Language = plain("justfile", &["#"]);
pub static ENV: Language = plain("env", &["#"]);
pub static SHELL: Language = plain("Shell", &["#"]);
pub static POWERSHELL: Language =
    Language { help_keywords: POWERSHELL_HELP, ..blocks("PowerShell", &["#"], &[("<#", "#>")]) };
// Batch files comment with REM in any case, or with a label that cannot be jumped to.
pub static BATCH: Language = plain("Batch", &["::", "REM", "rem", "Rem", "@REM", "@rem"]);
pub static RUST: Language = Language {
    nested: true,
    doc_lines: &["///", "//!"],
    doc_blocks: &["/**", "/*!"],
    ..blocks("Rust", &["//"], C_BLOCK)
};
pub static JAVASCRIPT: Language = Language { doc_blocks: &["/**"], ..blocks("JavaScript", &["//"], C_BLOCK) };
pub static TYPESCRIPT: Language = Language { doc_blocks: &["/**"], ..blocks("TypeScript", &["//"], C_BLOCK) };
pub static CSS: Language = blocks("CSS", &[], C_BLOCK);
pub static HTML: Language = blocks("HTML", &[], XML_BLOCK);
pub static XML: Language = blocks("XML", &[], XML_BLOCK);
pub static DJANGO_TEMPLATE: Language =
    blocks("Django template", &[], &[("<!--", "-->"), ("{#", "#}"), ("{% comment %}", "{% endcomment %}")]);
pub static GETTEXT: Language = plain("gettext", &["#"]);
// JSON carries no comments. A block marker would open on every glob like `**/*.py` in a string.
pub static JSON: Language = plain("JSON", &[]);
pub static JSONC: Language = plain("JSON with comments", &["//"]);
pub static JSON_LINES: Language = plain("JSON Lines", &[]);
pub static CSV: Language = plain("CSV", &[]);
pub static LOG: Language = plain("Log", &[]);
pub static HCL: Language = blocks("HCL", &["#", "//"], C_BLOCK);
pub static DOCKERFILE: Language = plain("Dockerfile", &["#"]);
pub static SQL: Language = blocks("SQL", &["--"], C_BLOCK);
pub static MERMAID: Language = plain("Mermaid", &["%%"]);
pub static JINJA: Language = blocks("Jinja", &[], &[("{#", "#}")]);
pub static INI: Language = plain("INI", &["#", ";"]);
pub static GIT: Language = plain("git", &["#", ";"]);
pub static REGISTRY: Language = plain("Windows registry", &[";"]);
pub static CADDYFILE: Language = plain("Caddyfile", &["#"]);
pub static KDL: Language = Language { nested: true, ..blocks("KDL", &["//"], C_BLOCK) };
pub static LUA: Language = blocks("Lua", &["--"], &[("--[[", "]]")]);
pub static AUTOHOTKEY: Language = blocks("AutoHotkey", &[";"], C_BLOCK);
pub static EMACS_LISP: Language = plain("Emacs Lisp", &[";"]);
pub static MARKDOWN: Language = blocks("Markdown", &[], XML_BLOCK);
pub static TEXT: Language = Language { prose: true, ..plain("Text", &[]) };
pub static NOTEBOOK: Language = plain("Jupyter Notebook", &[]);
pub static UNKNOWN: Language = plain("unknown", &[]);

/// The family a language folds into unless the detailed view is asked for: a dialect counts with
/// the language it extends, the small configuration formats count together.
pub fn family(name: &'static str) -> &'static str {
    match name {
        "JSON with comments" | "JSON Lines" => "JSON",
        "Caddyfile" | "INI" | "git" | "Windows registry" | "KDL" | "env" => "Configuration",
        "Batch" => "Shell",
        _ => name,
    }
}

/// The language a Markdown fence or a notebook kernel names.
pub fn by_tag(tag: &str) -> &'static Language {
    match tag {
        "python" | "py" | "python3" => &PYTHON,
        "toml" => &TOML,
        "yaml" | "yml" => &YAML,
        "just" | "justfile" => &JUSTFILE,
        "env" | "dotenv" => &ENV,
        "bash" | "sh" | "shell" | "zsh" => &SHELL,
        "powershell" | "pwsh" | "ps1" => &POWERSHELL,
        "bat" | "batch" | "cmd" => &BATCH,
        "rust" | "rs" => &RUST,
        "javascript" | "js" => &JAVASCRIPT,
        "typescript" | "ts" | "tsx" => &TYPESCRIPT,
        "css" => &CSS,
        "html" => &HTML,
        "xml" | "svg" => &XML,
        "django" | "htmldjango" => &DJANGO_TEMPLATE,
        "json" => &JSON,
        "jsonl" | "ndjson" => &JSON_LINES,
        "jsonc" => &JSONC,
        "csv" | "tsv" => &CSV,
        "log" => &LOG,
        "hcl" | "terraform" | "tf" => &HCL,
        "dockerfile" | "containerfile" => &DOCKERFILE,
        "sql" => &SQL,
        "mermaid" => &MERMAID,
        "jinja" | "jinja2" | "j2" => &JINJA,
        "po" | "gettext" => &GETTEXT,
        "ini" | "conf" => &INI,
        "gitconfig" | "gitignore" => &GIT,
        "caddy" | "caddyfile" => &CADDYFILE,
        "kdl" => &KDL,
        "lua" => &LUA,
        "ahk" | "autohotkey" => &AUTOHOTKEY,
        "elisp" | "emacs-lisp" => &EMACS_LISP,
        "markdown" | "md" => &MARKDOWN,
        "text" | "txt" | "plain" | "plaintext" => &TEXT,
        // A fence may name the file it shows: ```base.djt.html.
        _ => language_of(tag),
    }
}

fn by_suffix(suffix: &str) -> &'static Language {
    match suffix {
        "py" | "pyi" => &PYTHON,
        "toml" => &TOML,
        "yml" | "yaml" => &YAML,
        "just" => &JUSTFILE,
        "env" => &ENV,
        "sh" | "bash" | "zsh" => &SHELL,
        "ps1" | "psm1" | "psd1" => &POWERSHELL,
        "bat" | "cmd" => &BATCH,
        "rs" => &RUST,
        "js" | "mjs" | "cjs" | "jsx" => &JAVASCRIPT,
        "ts" | "mts" | "cts" | "tsx" => &TYPESCRIPT,
        "css" => &CSS,
        "html" | "htm" => &HTML,
        "xml" | "svg" | "drawio" | "xaml" | "plist" => &XML,
        "po" | "pot" => &GETTEXT,
        "json" | "geojson" | "schema" => &JSON,
        "jsonl" | "ndjson" => &JSON_LINES,
        "csv" | "tsv" => &CSV,
        "log" => &LOG,
        "tf" | "tfvars" | "hcl" => &HCL,
        "dockerfile" | "containerfile" => &DOCKERFILE,
        "sql" => &SQL,
        "mmd" | "mermaid" => &MERMAID,
        "jsonc" | "code-workspace" => &JSONC,
        "ini" | "cfg" | "conf" | "wslconfig" | "editorconfig" | "properties" => &INI,
        // systemd units
        "service" | "timer" | "socket" | "mount" => &INI,
        "gitconfig" | "gitignore" | "gitattributes" | "gitmodules" => &GIT,
        "reg" => &REGISTRY,
        "caddyfile" => &CADDYFILE,
        "kdl" => &KDL,
        "lua" => &LUA,
        "ahk" => &AUTOHOTKEY,
        "el" | "emacs" => &EMACS_LISP,
        "md" => &MARKDOWN,
        "txt" => &TEXT,
        "ipynb" => &NOTEBOOK,
        _ => &UNKNOWN,
    }
}

fn by_name(name: &str) -> Option<&'static Language> {
    Some(match name {
        "justfile" | "Justfile" | ".justfile" => &JUSTFILE,
        ".env" => &ENV,
        ".bashrc" | ".bash_profile" | ".bash_logout" | ".profile" | ".zshrc" | ".zprofile" => &SHELL,
        "Caddyfile" => &CADDYFILE,
        "Dockerfile" | "Containerfile" => &DOCKERFILE,
        "LICENSE" | "LICENCE" | "COPYING" | "NOTICE" | "AUTHORS" => &TEXT,
        _ => return None,
    })
}

// Layered names carry their layer as the suffix: `.justfile.library`, `.env.example`.
const BY_PREFIX: &[(&str, &Language)] =
    &[(".justfile.", &JUSTFILE), (".env.", &ENV), ("Dockerfile.", &DOCKERFILE), ("Containerfile.", &DOCKERFILE)];

/// Suffixes that wrap a template around a file of another language — `pyproject.toml.jinja`,
/// `admin.py-tpl` — and the language the file takes when what they wrap is unknown.
const TEMPLATE_SUFFIXES: &[(&str, &Language)] = &[
    (".jinja", &JINJA),
    (".jinja2", &JINJA),
    (".j2", &JINJA),
    (".djt", &DJANGO_TEMPLATE),
    ("-tpl", &UNKNOWN),
    (".template", &UNKNOWN),
];

/// The name inside a template suffix and the suffix's own language, None when the name has no such suffix.
pub fn unwrap_template(name: &str) -> Option<(&str, &'static Language)> {
    TEMPLATE_SUFFIXES.iter().find_map(|(suffix, language)| Some((name.strip_suffix(suffix)?, *language)))
}

/// Find a file's language by its name first, then its prefix, then what a template suffix wraps, then its suffix.
pub fn language_of(path: &str) -> &'static Language {
    let (_, mut name) = path.rsplit_once('/').unwrap_or(("", path));
    // What the innermost template suffix says, should the file it wraps be unknown.
    let mut wrapper: &'static Language = &UNKNOWN;
    loop {
        if let Some(language) = by_name(name) {
            return language;
        }
        if name.ends_with(".djt.html") {
            return &DJANGO_TEMPLATE;
        }
        if let Some((_, language)) = BY_PREFIX.iter().find(|(prefix, _)| name.starts_with(prefix)) {
            return language;
        }
        let Some((inner, own)) = unwrap_template(name) else { break };
        name = inner;
        if *own != UNKNOWN {
            wrapper = own;
        }
    }
    // A dotfile like `.gitconfig` is all suffix; `name.ext` takes the part after the last dot.
    let language = name.rsplit_once('.').map_or(&UNKNOWN, |(_, suffix)| by_suffix(&suffix.to_ascii_lowercase()));
    if *language == UNKNOWN { wrapper } else { language }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_wins_over_the_suffix() {
        assert_eq!(language_of(".env"), &ENV);
        assert_eq!(language_of("scratch/.justfile.library"), &JUSTFILE);
        assert_eq!(language_of("templates/base.djt.html"), &DJANGO_TEMPLATE);
    }

    #[test]
    fn a_dotfile_is_found_by_what_follows_its_dot() {
        assert_eq!(language_of("common/git/.gitconfig"), &GIT);
        assert_eq!(language_of(".wslconfig"), &INI);
    }

    #[test]
    fn the_suffix_is_read_without_regard_to_case() {
        assert_eq!(language_of("caddy/EDGE.Caddyfile"), &CADDYFILE);
        assert_eq!(language_of("README.MD"), &MARKDOWN);
    }

    #[test]
    fn a_template_suffix_counts_the_file_under_the_language_it_wraps() {
        assert_eq!(language_of("template/pyproject.toml.jinja"), &TOML);
        assert_eq!(language_of("app_template/admin.py-tpl"), &PYTHON);
        assert_eq!(language_of("envs/backups.env.template"), &ENV);
        assert_eq!(language_of("caddy/Caddyfile-custom-dns.j2"), &JINJA);
        assert_eq!(language_of("envs/develop.template.djt"), &DJANGO_TEMPLATE);
    }

    #[test]
    fn data_files_have_languages_of_their_own() {
        assert_eq!(language_of("queue/state/messages.jsonl"), &JSON_LINES);
        assert_eq!(language_of("datasets/income.csv"), &CSV);
        assert_eq!(language_of("logs/history-2025-06.log"), &LOG);
    }

    #[test]
    fn infrastructure_and_schema_files_are_known() {
        assert_eq!(language_of("infrastructure.tf"), &HCL);
        assert_eq!(language_of("compose/Dockerfile"), &DOCKERFILE);
        assert_eq!(language_of("postgres/postgres.Dockerfile"), &DOCKERFILE);
        assert_eq!(language_of(".devops/systemd/app.service"), &INI);
        assert_eq!(language_of("core/schemas/connection.schema"), &JSON);
    }

    #[test]
    fn a_fence_naming_a_file_takes_that_file_s_language() {
        assert_eq!(by_tag("variables.auto.tfvars"), &HCL);
        assert_eq!(by_tag("base.djt.html"), &DJANGO_TEMPLATE);
        assert_eq!(by_tag("tree"), &UNKNOWN);
    }

    #[test]
    fn template_suffixes_unwrap_to_any_depth() {
        assert_eq!(language_of(&format!("settings.toml{}", ".j2".repeat(1_000_000))), &TOML);
        assert_eq!(language_of(&format!("admin.py{}", "-tpl".repeat(1_000_000))), &PYTHON);
        assert_eq!(language_of("note.template.j2"), &JINJA);
        assert_eq!(language_of("note.j2.template"), &JINJA);
    }

    #[test]
    fn a_file_without_a_known_name_or_suffix_is_unknown() {
        assert_eq!(language_of("htoprc"), &UNKNOWN);
        assert_eq!(language_of("archive.xyz"), &UNKNOWN);
    }
}
