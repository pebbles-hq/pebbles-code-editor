//! Snippets: built-in per-language snippets plus user-defined ones (VS Code snippet files),
//! offered in the completion list as [`CompletionKind::Snippet`] items. Bodies use VS Code
//! snippet syntax (`$1`, `${1:default}`, `$0`); `\t` is one indent unit and continuation
//! lines take the caret line's indentation.

use std::rc::Rc;

use crate::providers::{CompletionContext, CompletionItem, CompletionKind, CompletionProvider};

/// A snippet: typing `prefix` offers `body` in the completion list.
#[derive(Clone, Debug, PartialEq)]
pub struct Snippet {
    /// The trigger word shown and matched in the completion list.
    pub prefix: String,
    /// The body in VS Code snippet syntax.
    pub body: String,
    /// A short description shown as the item detail.
    pub description: String,
}

impl Snippet {
    /// A snippet with `prefix`, `body` and `description`.
    pub fn new(
        prefix: impl Into<String>,
        body: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Snippet {
            prefix: prefix.into(),
            body: body.into(),
            description: description.into(),
        }
    }
}

/// The built-in snippets for a language (by [`Language::name`](crate::Language::name),
/// grammar id or extension); empty for languages without any.
pub fn builtin_snippets(language: &str) -> Vec<Snippet> {
    let s = Snippet::new;
    match language.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => vec![
            s("fn", "fn ${1:name}(${2}) {\n\t$0\n}", "Function"),
            s(
                "pfn",
                "pub fn ${1:name}(${2}) {\n\t$0\n}",
                "Public function",
            ),
            s("if", "if ${1:condition} {\n\t$0\n}", "If"),
            s(
                "ifl",
                "if let ${1:Some(x)} = ${2:value} {\n\t$0\n}",
                "If let",
            ),
            s("for", "for ${1:item} in ${2:iter} {\n\t$0\n}", "For loop"),
            s("while", "while ${1:condition} {\n\t$0\n}", "While loop"),
            s("loop", "loop {\n\t$0\n}", "Loop"),
            s("match", "match ${1:value} {\n\t${2:_} => $0,\n}", "Match"),
            s("struct", "struct ${1:Name} {\n\t$0\n}", "Struct"),
            s("enum", "enum ${1:Name} {\n\t$0\n}", "Enum"),
            s("impl", "impl ${1:Type} {\n\t$0\n}", "Impl block"),
            s("trait", "trait ${1:Name} {\n\t$0\n}", "Trait"),
            s(
                "test",
                "#[test]\nfn ${1:name}() {\n\t$0\n}",
                "Test function",
            ),
            s(
                "modtest",
                "#[cfg(test)]\nmod tests {\n\tuse super::*;\n\n\t$0\n}",
                "Test module",
            ),
        ],
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "typescript" | "ts" | "tsx" | "mts"
        | "cts" => vec![
            s(
                "function",
                "function ${1:name}(${2}) {\n\t$0\n}",
                "Function",
            ),
            s("if", "if (${1:condition}) {\n\t$0\n}", "If"),
            s(
                "ifelse",
                "if (${1:condition}) {\n\t$2\n} else {\n\t$0\n}",
                "If-else",
            ),
            s(
                "for",
                "for (let ${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}",
                "For loop",
            ),
            s(
                "forof",
                "for (const ${1:item} of ${2:items}) {\n\t$0\n}",
                "For-of loop",
            ),
            s("while", "while (${1:condition}) {\n\t$0\n}", "While loop"),
            s(
                "class",
                "class ${1:Name} {\n\tconstructor(${2}) {\n\t\t$0\n\t}\n}",
                "Class",
            ),
            s(
                "trycatch",
                "try {\n\t$1\n} catch (${2:error}) {\n\t$0\n}",
                "Try-catch",
            ),
            s("log", "console.log($0);", "Log to the console"),
        ],
        "python" | "py" | "pyi" | "pyw" => vec![
            s("def", "def ${1:name}(${2}):\n\t${0:pass}", "Function"),
            s(
                "class",
                "class ${1:Name}:\n\tdef __init__(self${2}):\n\t\t${0:pass}",
                "Class",
            ),
            s("if", "if ${1:condition}:\n\t${0:pass}", "If"),
            s(
                "for",
                "for ${1:item} in ${2:items}:\n\t${0:pass}",
                "For loop",
            ),
            s("while", "while ${1:condition}:\n\t${0:pass}", "While loop"),
            s(
                "try",
                "try:\n\t${1:pass}\nexcept ${2:Exception} as ${3:e}:\n\t${0:raise}",
                "Try-except",
            ),
            s("with", "with ${1:expr} as ${2:var}:\n\t${0:pass}", "With"),
            s(
                "main",
                "if __name__ == \"__main__\":\n\t${0:main()}",
                "Main guard",
            ),
        ],
        "go" | "golang" => vec![
            s("func", "func ${1:name}(${2}) {\n\t$0\n}", "Function"),
            s("if", "if ${1:condition} {\n\t$0\n}", "If"),
            s("iferr", "if err != nil {\n\treturn ${0:err}\n}", "If error"),
            s(
                "for",
                "for ${1:i} := 0; $1 < ${2:n}; $1++ {\n\t$0\n}",
                "For loop",
            ),
            s(
                "forr",
                "for ${1:_}, ${2:v} := range ${3:items} {\n\t$0\n}",
                "For range",
            ),
            s("struct", "type ${1:Name} struct {\n\t$0\n}", "Struct"),
        ],
        "c" | "h" | "c++" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => vec![
            s("if", "if (${1:condition}) {\n\t$0\n}", "If"),
            s(
                "for",
                "for (int ${1:i} = 0; $1 < ${2:n}; $1++) {\n\t$0\n}",
                "For loop",
            ),
            s("while", "while (${1:condition}) {\n\t$0\n}", "While loop"),
            s("struct", "struct ${1:Name} {\n\t$0\n};", "Struct"),
            s(
                "main",
                "int main(int argc, char *argv[]) {\n\t$0\n\treturn 0;\n}",
                "Main",
            ),
        ],
        "html" | "htm" => vec![
            s(
                "html5",
                "<!DOCTYPE html>\n<html lang=\"${1:en}\">\n<head>\n\t<meta charset=\"UTF-8\">\n\t<title>${2:Document}</title>\n</head>\n<body>\n\t$0\n</body>\n</html>",
                "HTML5 document",
            ),
            s("div", "<div>$0</div>", "Div"),
            s("a", "<a href=\"$1\">$0</a>", "Link"),
            s("script", "<script>$0</script>", "Script"),
        ],
        "css" | "scss" | "less" => vec![
            s(
                "media",
                "@media (${1:max-width: 600px}) {\n\t$0\n}",
                "Media query",
            ),
            s(
                "flex",
                "display: flex;\njustify-content: ${1:center};\nalign-items: ${2:center};$0",
                "Flex container",
            ),
        ],
        "shell" | "bash" | "sh" | "zsh" => vec![
            s("if", "if [ ${1:condition} ]; then\n\t$0\nfi", "If"),
            s(
                "for",
                "for ${1:item} in ${2:items}; do\n\t$0\ndone",
                "For loop",
            ),
            s(
                "while",
                "while ${1:condition}; do\n\t$0\ndone",
                "While loop",
            ),
            s("func", "${1:name}() {\n\t$0\n}", "Function"),
            s(
                "shebang",
                "#!/usr/bin/env bash\nset -euo pipefail\n$0",
                "Shebang",
            ),
        ],
        "markdown" | "md" => vec![
            s("code", "```${1:lang}\n$0\n```", "Code block"),
            s("link", "[${1:text}](${2:url})$0", "Link"),
            s(
                "table",
                "| ${1:A} | ${2:B} |\n| --- | --- |\n| $3 | $0 |",
                "Table",
            ),
        ],
        _ => Vec::new(),
    }
}

/// Wrap `base` (the host provider, word completion, or none) so `snippets` are offered too.
pub(crate) fn with_snippets(
    base: Option<CompletionProvider>,
    snippets: Rc<Vec<Snippet>>,
    unit: String,
) -> Option<CompletionProvider> {
    if snippets.is_empty() {
        return base;
    }
    Some(Rc::new(move |cx: &CompletionContext| {
        let mut items = base.as_ref().map(|b| b(cx)).unwrap_or_default();
        items.extend(snippets.iter().map(|s| {
            CompletionItem::new(s.prefix.clone(), CompletionKind::Snippet)
                .insert(s.body.replace('\t', &unit))
                .detail(s.description.clone())
        }));
        items
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_exist_for_common_languages() {
        for l in [
            "Rust", "ts", "python", "go", "c", "html", "css", "bash", "markdown",
        ] {
            assert!(!builtin_snippets(l).is_empty(), "{l}");
        }
        assert!(builtin_snippets("Plain Text").is_empty());
    }
}
