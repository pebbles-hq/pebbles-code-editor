//! TextMate-like scope stacks for tokens, so TextMate-style themes (VS Code `tokenColors`)
//! can match descendant selectors such as `source.rust keyword` or
//! `meta.function entity.name.function`.
//!
//! A scope stack is a space-separated list, outermost first: the language root scope
//! (`source.rust`), any structural `meta.*` scopes, then the token's own scope
//! (`storage.type.function.rust`). Stacks are interned process-wide into small
//! [`ScopeId`]s carried on each [`Token`](super::Token); `0` means "no scope".

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// An interned scope stack (`0` = none).
pub type ScopeId = u16;

struct Interner {
    names: Vec<Arc<str>>,
    ids: HashMap<Arc<str>, ScopeId>,
}

fn interner() -> &'static Mutex<Interner> {
    static I: OnceLock<Mutex<Interner>> = OnceLock::new();
    I.get_or_init(|| {
        Mutex::new(Interner {
            names: vec![Arc::from("")],
            ids: HashMap::new(),
        })
    })
}

/// Intern a scope stack. Returns `0` when the table is full (65 535 distinct stacks).
pub fn intern_scope(stack: &str) -> ScopeId {
    if stack.is_empty() {
        return 0;
    }
    let mut i = interner().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&id) = i.ids.get(stack) {
        return id;
    }
    let Ok(id) = ScopeId::try_from(i.names.len()) else {
        return 0;
    };
    let name: Arc<str> = Arc::from(stack);
    i.names.push(name.clone());
    i.ids.insert(name, id);
    id
}

/// The scope stack of `id` (`None` for `0` / unknown ids).
pub fn scope_name(id: ScopeId) -> Option<Arc<str>> {
    if id == 0 {
        return None;
    }
    let i = interner().lock().unwrap_or_else(|e| e.into_inner());
    i.names.get(usize::from(id)).cloned()
}

/// The root scope of a language id, as VS Code's grammars name it.
pub fn root_scope(lang: &str) -> String {
    match lang {
        "markdown" | "markdown_inline" => "text.html.markdown".into(),
        "html" => "text.html.basic".into(),
        "bash" => "source.shell".into(),
        "javascript" => "source.js".into(),
        "typescript" => "source.ts".into(),
        "tsx" => "source.tsx".into(),
        "python" => "source.python".into(),
        "cpp" => "source.cpp".into(),
        other => format!("source.{other}"),
    }
}

/// The TextMate scope a tree-sitter highlight capture corresponds to (VS Code's standard
/// grammars' naming), without the language suffix. `None` = the capture carries no colour.
pub fn textmate_scope(capture: &str) -> Option<&'static str> {
    if capture.starts_with('_') || capture.starts_with("injection") {
        return None;
    }
    Some(match capture {
        "keyword.function" => "storage.type.function",
        "keyword.type" | "storageclass" | "keyword.storage" | "keyword.storage.type" => {
            "storage.type"
        }
        "keyword.modifier" | "keyword.storage.modifier" | "type.qualifier" => "storage.modifier",
        "keyword.return" => "keyword.control.return",
        "keyword.import" | "include" | "keyword.control.import" => "keyword.control.import",
        "keyword.conditional" | "conditional" | "keyword.conditional.ternary" => {
            "keyword.control.conditional"
        }
        "keyword.repeat" | "repeat" => "keyword.control.loop",
        "keyword.exception" | "exception" => "keyword.control.exception",
        "keyword.coroutine" => "keyword.control.flow",
        "keyword.operator" | "operator" => "keyword.operator",
        "keyword.directive" | "preproc" | "define" | "keyword.directive.define" => {
            "keyword.control.directive"
        }
        "keyword.debug" => "keyword.other.debug",
        c if c.starts_with("keyword.control") => "keyword.control",
        c if c.starts_with("keyword") => "keyword.other",
        "type.builtin" => "support.type.primitive",
        "type.definition" => "entity.name.type.definition",
        c if c.starts_with("type") => "entity.name.type",
        "constructor" => "entity.name.type.constructor",
        "function.macro" | "macro" => "entity.name.function.macro",
        "function.builtin" => "support.function",
        "function.method" | "method" | "function.method.call" => "entity.name.function.method",
        c if c.starts_with("function") => "entity.name.function",
        "string.escape" | "escape" => "constant.character.escape",
        "string.special.regex" | "string.regexp" | "string.regex" => "string.regexp",
        "string.special.key" => "support.type.property-name",
        c if c.starts_with("string.special") => "string.other",
        "character" | "string.character" => "string.quoted.single",
        c if c.starts_with("string") => "string.quoted.double",
        "number" | "float" | "number.float" => "constant.numeric",
        "boolean" | "constant.builtin.boolean" => "constant.language.boolean",
        "constant.builtin" => "constant.language",
        c if c.starts_with("constant") => "variable.other.constant",
        "comment.documentation" | "comment.doc" => "comment.block.documentation",
        c if c.starts_with("comment") => "comment.line",
        "attribute" | "decorator" | "annotation" | "attribute.builtin" => {
            "entity.other.attribute-name"
        }
        "property" | "field" | "variable.member" | "property.definition" => {
            "variable.other.property"
        }
        "variable.builtin" => "variable.language",
        "variable.parameter" | "parameter" => "variable.parameter",
        c if c.starts_with("variable") => "variable.other",
        "punctuation.bracket" => "punctuation.section.brackets",
        "punctuation.delimiter" => "punctuation.separator",
        "punctuation.special" => "punctuation.special",
        c if c.starts_with("punctuation") || c == "delimiter" => "punctuation",
        "tag.attribute" => "entity.other.attribute-name",
        "tag.delimiter" => "punctuation.definition.tag",
        c if c.starts_with("tag") => "entity.name.tag",
        "label" => "entity.name.label",
        "module" | "namespace" => "entity.name.namespace",
        "markup.heading" | "text.title" => "markup.heading",
        "markup.link.url" | "text.uri" | "markup.link" => "markup.underline.link",
        "markup.link.label" | "text.reference" => "string.other.link",
        "markup.italic" | "text.emphasis" => "markup.italic",
        "markup.bold" | "markup.strong" | "text.strong" => "markup.bold",
        "markup.strikethrough" | "text.strike" => "markup.strikethrough",
        "markup.raw" | "text.literal" => "markup.inline.raw",
        "markup.raw.block" => "markup.fenced_code.block",
        "markup.quote" => "markup.quote",
        "markup.list" => "punctuation.definition.list",
        c if c.starts_with("markup.heading") => "markup.heading",
        "embedded" | "none" | "spell" | "nospell" | "conceal" => return None,
        _ => return None,
    })
}

/// The `meta.*` scope a syntax node kind opens (functions, classes / structs, …).
pub fn meta_scope(node_kind: &str) -> Option<&'static str> {
    Some(match node_kind {
        "function_item"
        | "function_signature_item"
        | "function_declaration"
        | "function_definition"
        | "method_definition"
        | "method_declaration"
        | "function_expression"
        | "arrow_function"
        | "generator_function_declaration"
        | "closure_expression" => "meta.function",
        "struct_item"
        | "enum_item"
        | "union_item"
        | "trait_item"
        | "impl_item"
        | "class_declaration"
        | "class_definition"
        | "class_specifier"
        | "struct_specifier"
        | "interface_declaration"
        | "type_declaration" => "meta.class",
        "attribute_item" | "inner_attribute_item" | "decorator" => "meta.attribute",
        "use_declaration" | "import_statement" | "import_from_statement" | "import_declaration" => {
            "meta.import"
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_round_trips() {
        let a = intern_scope("source.rust keyword.other.rust");
        assert_ne!(a, 0);
        assert_eq!(a, intern_scope("source.rust keyword.other.rust"));
        assert_eq!(
            scope_name(a).as_deref(),
            Some("source.rust keyword.other.rust")
        );
        assert_eq!(intern_scope(""), 0);
        assert!(scope_name(0).is_none());
    }

    #[test]
    fn captures_map_to_textmate_scopes() {
        assert_eq!(textmate_scope("keyword"), Some("keyword.other"));
        assert_eq!(
            textmate_scope("keyword.function"),
            Some("storage.type.function")
        );
        assert_eq!(
            textmate_scope("function.method"),
            Some("entity.name.function.method")
        );
        assert_eq!(textmate_scope("variable"), Some("variable.other"));
        assert_eq!(textmate_scope("injection.content"), None);
        assert_eq!(root_scope("rust"), "source.rust");
        assert_eq!(root_scope("markdown"), "text.html.markdown");
    }
}
