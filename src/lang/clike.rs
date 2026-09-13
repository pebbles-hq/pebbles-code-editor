//! The C-family highlighter: one [`Grammar`]-parameterized scanner ([`CLike`]) shared by
//! JavaScript, TypeScript, Go, C/C++, C#, Java, Kotlin, Swift, PHP, Ruby, Shell, YAML, and
//! TOML. Add a curly-brace/hash-comment language with the `grammar!` macro.

use super::scan::{Scan, is_ident_continue, is_ident_start};
use super::{Language, Token, TokenKind};

/// The vocabulary that distinguishes one C-family language from another.
pub struct Grammar {
    /// The language's display name (shown in the status bar).
    pub name: &'static str,
    /// Reserved keywords, highlighted as [`super::TokenKind::Keyword`].
    pub keywords: &'static [&'static str],
    /// Built-in type names, highlighted as [`super::TokenKind::Type`].
    pub types: &'static [&'static str],
    /// Built-in constants (`true`, `null`, …), highlighted as [`super::TokenKind::Constant`].
    pub constants: &'static [&'static str],
    /// `true` if `#` starts a line comment (unused for C-family; see [`super::Python`]).
    pub hash_comments: bool,
}

/// A C-family highlighter driven by a [`Grammar`]. Shared by JS/TS/Go/C/Java and easy
/// to instantiate for any curly-brace language.
pub struct CLike(pub &'static Grammar);

impl Language for CLike {
    fn name(&self) -> &str {
        self.0.name
    }
    fn line_comment(&self) -> Option<&str> {
        // Hash-comment languages (Python-family via CLike) use `#`; C-family use `//`.
        Some(if self.0.hash_comments { "#" } else { "//" })
    }
    fn highlight(&self, src: &str) -> Vec<Token> {
        let g = self.0;
        let mut out = Vec::new();
        let mut sc = Scan::new(src);
        while !sc.done() {
            let b = sc.peek();
            let start = sc.pos;
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => sc.bump(),
                b'#' if g.hash_comments => {
                    while !sc.done() && sc.peek() != b'\n' {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                b'/' if sc.peek2() == b'/' => {
                    while !sc.done() && sc.peek() != b'\n' {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                b'/' if sc.peek2() == b'*' => {
                    sc.bump();
                    sc.bump();
                    while !sc.done() && !(sc.peek() == b'*' && sc.peek2() == b'/') {
                        sc.bump();
                    }
                    sc.bump();
                    sc.bump();
                    out.push(Token::new(start, sc.pos - start, TokenKind::Comment));
                }
                b'"' | b'\'' | b'`' => {
                    let q = b;
                    sc.bump();
                    while !sc.done() {
                        let c = sc.peek();
                        if c == b'\\' {
                            sc.bump();
                            sc.bump();
                            continue;
                        }
                        sc.bump();
                        if c == q {
                            break;
                        }
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Str));
                }
                b'0'..=b'9' => {
                    while !sc.done()
                        && (sc.peek().is_ascii_alphanumeric()
                            || sc.peek() == b'.'
                            || sc.peek() == b'_')
                    {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Number));
                }
                _ if is_ident_start(b) => {
                    while is_ident_continue(sc.peek()) {
                        sc.bump();
                    }
                    let word = &src[start..sc.pos];
                    let kind = if g.keywords.contains(&word) {
                        TokenKind::Keyword
                    } else if g.constants.contains(&word) {
                        TokenKind::Constant
                    } else if g.types.contains(&word)
                        || word.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        TokenKind::Type
                    } else if sc.peek() == b'(' {
                        TokenKind::Function
                    } else {
                        TokenKind::Plain
                    };
                    if kind != TokenKind::Plain {
                        out.push(Token::new(start, sc.pos - start, kind));
                    }
                }
                _ if b.is_ascii_punctuation() => {
                    while !sc.done()
                        && sc.peek().is_ascii_punctuation()
                        && !matches!(sc.peek(), b'"' | b'\'' | b'`')
                    {
                        sc.bump();
                    }
                    out.push(Token::new(start, sc.pos - start, TokenKind::Punctuation));
                }
                _ => sc.bump(),
            }
            if sc.pos == start {
                sc.bump();
            }
        }
        out
    }
}

macro_rules! grammar {
    ($fname:ident, $name:literal, kw = [$($kw:literal),* $(,)?], ty = [$($ty:literal),* $(,)?], c = [$($cn:literal),* $(,)?], hash = $hash:literal) => {
        #[doc = concat!("A ", $name, " highlighter.")]
        pub fn $fname() -> CLike {
            static G: Grammar = Grammar {
                name: $name,
                keywords: &[$($kw),*],
                types: &[$($ty),*],
                constants: &[$($cn),*],
                hash_comments: $hash,
            };
            CLike(&G)
        }
    };
}

grammar!(
    javascript,
    "JavaScript",
    kw = [
        "const",
        "let",
        "var",
        "function",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "new",
        "class",
        "extends",
        "super",
        "this",
        "typeof",
        "instanceof",
        "in",
        "of",
        "try",
        "catch",
        "finally",
        "throw",
        "async",
        "await",
        "yield",
        "delete",
        "void",
        "export",
        "import",
        "from",
        "as",
        "default",
        "static",
        "get",
        "set"
    ],
    ty = [],
    c = ["true", "false", "null", "undefined", "NaN", "Infinity"],
    hash = false
);

grammar!(
    typescript,
    "TypeScript",
    kw = [
        "const",
        "let",
        "var",
        "function",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "new",
        "class",
        "extends",
        "implements",
        "interface",
        "enum",
        "type",
        "super",
        "this",
        "typeof",
        "instanceof",
        "keyof",
        "in",
        "of",
        "try",
        "catch",
        "finally",
        "throw",
        "async",
        "await",
        "yield",
        "export",
        "import",
        "from",
        "as",
        "default",
        "public",
        "private",
        "protected",
        "readonly",
        "static",
        "abstract",
        "declare",
        "namespace",
        "get",
        "set"
    ],
    ty = [
        "string", "number", "boolean", "any", "void", "unknown", "never", "object", "symbol",
        "bigint"
    ],
    c = ["true", "false", "null", "undefined"],
    hash = false
);

grammar!(
    go,
    "Go",
    kw = [
        "package",
        "import",
        "func",
        "var",
        "const",
        "type",
        "struct",
        "interface",
        "map",
        "chan",
        "go",
        "defer",
        "return",
        "if",
        "else",
        "for",
        "range",
        "switch",
        "case",
        "default",
        "break",
        "continue",
        "fallthrough",
        "select",
        "goto"
    ],
    ty = [
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
        "float32", "float64", "string", "bool", "byte", "rune", "error", "any"
    ],
    c = ["true", "false", "nil", "iota"],
    hash = false
);

grammar!(
    c_lang,
    "C",
    kw = [
        "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
        "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
        "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef", "union",
        "unsigned", "void", "volatile", "while"
    ],
    ty = [
        "size_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "int8_t", "int16_t", "int32_t",
        "int64_t", "bool", "FILE"
    ],
    c = ["NULL", "true", "false"],
    hash = false
);

grammar!(
    java,
    "Java",
    kw = [
        "public",
        "private",
        "protected",
        "class",
        "interface",
        "enum",
        "extends",
        "implements",
        "import",
        "package",
        "static",
        "final",
        "abstract",
        "void",
        "new",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "try",
        "catch",
        "finally",
        "throw",
        "throws",
        "this",
        "super",
        "synchronized",
        "volatile",
        "transient",
        "instanceof"
    ],
    ty = [
        "int", "long", "short", "byte", "char", "boolean", "float", "double", "String", "Object",
        "void", "var"
    ],
    c = ["true", "false", "null"],
    hash = false
);

grammar!(
    cpp,
    "C++",
    kw = [
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "constexpr",
        "continue",
        "decltype",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "return",
        "sizeof",
        "static",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "try",
        "typedef",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
        "concept",
        "requires",
        "co_await",
        "co_return",
        "co_yield"
    ],
    ty = [
        "int", "long", "short", "char", "bool", "float", "double", "void", "unsigned", "signed",
        "size_t", "wchar_t", "string", "vector", "map", "set", "auto"
    ],
    c = ["true", "false", "nullptr", "NULL"],
    hash = false
);

grammar!(
    csharp,
    "C#",
    kw = [
        "using",
        "namespace",
        "class",
        "struct",
        "interface",
        "enum",
        "record",
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "readonly",
        "const",
        "abstract",
        "sealed",
        "virtual",
        "override",
        "new",
        "return",
        "if",
        "else",
        "for",
        "foreach",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "try",
        "catch",
        "finally",
        "throw",
        "async",
        "await",
        "yield",
        "var",
        "get",
        "set",
        "in",
        "out",
        "ref",
        "params",
        "is",
        "as",
        "this",
        "base",
        "typeof",
        "nameof"
    ],
    ty = [
        "int", "long", "short", "byte", "char", "bool", "float", "double", "decimal", "string",
        "object", "void", "var", "dynamic"
    ],
    c = ["true", "false", "null"],
    hash = false
);

grammar!(
    kotlin,
    "Kotlin",
    kw = [
        "fun",
        "val",
        "var",
        "class",
        "object",
        "interface",
        "data",
        "sealed",
        "enum",
        "return",
        "if",
        "else",
        "for",
        "while",
        "do",
        "when",
        "break",
        "continue",
        "try",
        "catch",
        "finally",
        "throw",
        "import",
        "package",
        "public",
        "private",
        "protected",
        "internal",
        "open",
        "override",
        "abstract",
        "companion",
        "init",
        "constructor",
        "suspend",
        "in",
        "is",
        "as",
        "by",
        "lateinit",
        "vararg",
        "typealias"
    ],
    ty = [
        "Int", "Long", "Short", "Byte", "Char", "Boolean", "Float", "Double", "String", "Any",
        "Unit", "List", "Map", "Set", "Array"
    ],
    c = ["true", "false", "null", "this", "super"],
    hash = false
);

grammar!(
    swift,
    "Swift",
    kw = [
        "func",
        "let",
        "var",
        "class",
        "struct",
        "enum",
        "protocol",
        "extension",
        "return",
        "if",
        "else",
        "for",
        "in",
        "while",
        "repeat",
        "switch",
        "case",
        "default",
        "break",
        "continue",
        "guard",
        "defer",
        "do",
        "try",
        "catch",
        "throw",
        "throws",
        "rethrows",
        "import",
        "public",
        "private",
        "internal",
        "fileprivate",
        "open",
        "static",
        "final",
        "override",
        "init",
        "deinit",
        "self",
        "super",
        "some",
        "any",
        "where",
        "as",
        "is",
        "async",
        "await",
        "actor"
    ],
    ty = [
        "Int",
        "Double",
        "Float",
        "Bool",
        "String",
        "Character",
        "Array",
        "Dictionary",
        "Set",
        "Optional",
        "Any",
        "Void"
    ],
    c = ["true", "false", "nil"],
    hash = false
);

grammar!(
    php,
    "PHP",
    kw = [
        "function",
        "class",
        "interface",
        "trait",
        "extends",
        "implements",
        "public",
        "private",
        "protected",
        "static",
        "const",
        "abstract",
        "final",
        "return",
        "if",
        "else",
        "elseif",
        "for",
        "foreach",
        "while",
        "do",
        "switch",
        "case",
        "break",
        "continue",
        "try",
        "catch",
        "finally",
        "throw",
        "new",
        "use",
        "namespace",
        "echo",
        "print",
        "as",
        "instanceof",
        "global",
        "isset",
        "unset",
        "list",
        "array",
        "fn",
        "match",
        "yield"
    ],
    ty = [
        "int", "float", "string", "bool", "array", "object", "void", "mixed", "callable"
    ],
    c = ["true", "false", "null", "this"],
    hash = false
);

grammar!(
    ruby,
    "Ruby",
    kw = [
        "def",
        "class",
        "module",
        "return",
        "if",
        "elsif",
        "else",
        "unless",
        "case",
        "when",
        "while",
        "until",
        "for",
        "in",
        "do",
        "begin",
        "rescue",
        "ensure",
        "raise",
        "yield",
        "then",
        "end",
        "require",
        "require_relative",
        "attr_accessor",
        "attr_reader",
        "attr_writer",
        "new",
        "lambda",
        "proc",
        "next",
        "break",
        "redo",
        "retry",
        "and",
        "or",
        "not"
    ],
    ty = [],
    c = ["true", "false", "nil", "self", "__FILE__", "__LINE__"],
    hash = true
);

grammar!(
    bash,
    "Shell",
    kw = [
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
        "esac", "function", "return", "break", "continue", "local", "export", "readonly",
        "declare", "echo", "cd", "exit", "source", "alias", "unset", "set", "trap", "shift"
    ],
    ty = [],
    c = ["true", "false"],
    hash = true
);

grammar!(
    yaml,
    "YAML",
    kw = ["true", "false", "null", "yes", "no", "on", "off"],
    ty = [],
    c = ["true", "false", "null", "yes", "no"],
    hash = true
);

grammar!(
    toml,
    "TOML",
    kw = [],
    ty = [],
    c = ["true", "false"],
    hash = true
);
