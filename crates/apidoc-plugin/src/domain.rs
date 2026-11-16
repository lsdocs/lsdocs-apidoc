//! The domain's directives and roles in pages: object descriptions,
//! `module`, `auto`, and references.

use lsdocs_apidoc::index::{Resolved, split_name};
use lsdocs_apidoc::{Index, ItemKind, anchor};
use lsdocs_plugin_sdk::doctree::{Anchor, Custom, Document, Node, Span, Text};
use lsdocs_plugin_sdk::protocol::Diagnostic;
use serde_json::{Value, json};

use crate::render::{Renderer, api_item, code, link};
use crate::{Language, State, warning};

/// What a directive node of the domain is.
enum Directive {
    /// An object description: its kind.
    Object(ItemKind),
    /// `module` (anchored) or `currentmodule` (not).
    Module { anchored: bool },
    /// `auto`.
    Auto,
}

fn directive(language: &impl Language, kind: &str) -> Option<Directive> {
    let rest = kind.strip_prefix(language.domain())?.strip_prefix(':')?;
    match rest {
        "module" => Some(Directive::Module { anchored: true }),
        "currentmodule" => Some(Directive::Module { anchored: false }),
        "auto" => Some(Directive::Auto),
        object => language
            .objects()
            .into_iter()
            .find(|(name, _)| name == object)
            .map(|(_, kind)| Directive::Object(kind)),
    }
}

fn argument(c: &Custom) -> &str {
    c.attrs
        .get("argument")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn flag(c: &Custom, name: &str) -> bool {
    c.attrs.get("options").and_then(|o| o.get(name)).is_some()
}

fn option<'c>(c: &'c Custom, name: &str) -> Option<&'c str> {
    c.attrs.get("options")?.get(name)?.as_str()
}

/// The scope an object directive's item is in: its `:module:` option, or
/// the page's current scope.
fn object_scope(c: &Custom, scope: &[String]) -> Vec<String> {
    match option(c, "module") {
        Some(module) => split_name(module),
        None => scope.to_vec(),
    }
}

/// The items `document` describes by hand (object directives without
/// `:noindex:`), with their kinds.
pub(crate) fn described(
    language: &impl Language,
    document: &Document,
) -> Vec<(Vec<String>, ItemKind)> {
    fn walk(
        language: &impl Language,
        nodes: &[Node],
        scope: &mut Vec<String>,
        out: &mut Vec<(Vec<String>, ItemKind)>,
    ) {
        for node in nodes {
            let Node::Custom(c) = node else {
                walk(language, node.children(), scope, out);
                continue;
            };
            match directive(language, &c.kind) {
                Some(Directive::Module { anchored }) => {
                    *scope = split_name(argument(c));
                    if anchored && !flag(c, "noindex") {
                        out.push((scope.clone(), ItemKind::Module));
                    }
                    walk(language, &c.children, scope, out);
                }
                Some(Directive::Object(kind)) => {
                    let outer = object_scope(c, scope);
                    match language.describe(kind.clone(), argument(c), &outer) {
                        Some(item) => {
                            if !flag(c, "noindex") {
                                out.push((item.path.clone(), kind));
                            }
                            let mut inner = item.path.clone();
                            walk(language, &c.children, &mut inner, out);
                        }
                        None => walk(language, &c.children, scope, out),
                    }
                }
                Some(Directive::Auto) => {}
                None => walk(language, &c.children, scope, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(language, &document.children, &mut Vec::new(), &mut out);
    out
}

/// Expands the domain's directives in `document` and resolves its roles.
pub(crate) async fn apply<L: Language>(
    language: &L,
    state: &State,
    renderer: &Renderer<'_, L>,
    document: &mut Document,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let page = document.source.clone();
    // Object and module directives, with `auto` left as placeholders.
    let children = std::mem::take(&mut document.children);
    document.children = expand(language, children, &mut Vec::new(), diagnostics);
    // Each `auto`, rendered from the generated items.
    let mut autos = Vec::new();
    collect_autos(&document.children, &mut autos);
    let mut rendered = Vec::new();
    for (name, members, span, scope) in autos {
        let path = split_name(&name);
        let found = state.items.get(&path).or_else(|| {
            match state.index.resolve(language.domain(), &name, &scope, None) {
                Resolved::Found(entry) => state.items.get(&entry.path),
                _ => None,
            }
        });
        match found {
            Some(item) => {
                rendered.push(renderer.auto(item, members, &page, &state.index).await);
            }
            None => {
                diagnostics.push(warning(
                    span,
                    format!("{}:auto: no item named {name:?}", language.domain()),
                ));
                rendered.push(Vec::new());
            }
        }
    }
    diagnostics.extend(renderer.take_diagnostics());
    let mut rendered = rendered.into_iter();
    let children = std::mem::take(&mut document.children);
    document.children = replace_autos(children, &mut rendered);
    // References, in the scope they are written in.
    let mut children = std::mem::take(&mut document.children);
    resolve_scoped(
        language,
        &state.index,
        &mut children,
        &page,
        &mut Vec::new(),
        diagnostics,
    );
    document.children = children;
}

/// Object and module directives to their nodes; `auto` to placeholders
/// holding their name and scope.
fn expand(
    language: &impl Language,
    nodes: Vec<Node>,
    scope: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for mut node in nodes {
        let Node::Custom(c) = &mut node else {
            if let Some(children) = node.children_mut() {
                let expanded = expand(language, std::mem::take(children), scope, diagnostics);
                *children = expanded;
            }
            out.push(node);
            continue;
        };
        match directive(language, &c.kind) {
            Some(Directive::Module { anchored }) => {
                *scope = split_name(argument(c));
                if anchored && !flag(c, "noindex") {
                    out.push(Node::Anchor(Anchor {
                        span: c.span,
                        id: anchor(language.domain(), scope),
                    }));
                }
                // A marker: references after it are in the module.
                out.push(Node::Custom(Custom {
                    span: c.span,
                    kind: "api-scope".into(),
                    attrs: [("scope".to_owned(), json!(scope))].into(),
                    children: Vec::new(),
                }));
                let children = std::mem::take(&mut c.children);
                out.extend(expand(language, children, scope, diagnostics));
            }
            Some(Directive::Object(kind)) => {
                let outer = object_scope(c, scope);
                let Some(item) = language.describe(kind, argument(c), &outer) else {
                    diagnostics.push(warning(
                        c.span,
                        format!("{}: cannot read the signature {:?}", c.kind, argument(c)),
                    ));
                    continue;
                };
                let mut children = Vec::new();
                if !flag(c, "noindex") {
                    children.push(Node::Anchor(Anchor {
                        span: c.span,
                        id: anchor(language.domain(), &item.path),
                    }));
                }
                children.push(Node::CodeBlock(lsdocs_plugin_sdk::doctree::CodeBlock {
                    span: c.span,
                    language: Some(language.code_language().to_owned()),
                    info: None,
                    value: language.signature(&item),
                }));
                let mut inner = item.path.clone();
                children.extend(expand(
                    language,
                    std::mem::take(&mut c.children),
                    &mut inner,
                    diagnostics,
                ));
                let mut node = api_item(&item, children);
                *node.span_mut() = c.span;
                out.push(node);
            }
            Some(Directive::Auto) => {
                c.kind = "api-auto".into();
                c.attrs.insert("scope".into(), json!(scope));
                out.push(node);
            }
            None => {
                let children = std::mem::take(&mut c.children);
                c.children = expand(language, children, scope, diagnostics);
                out.push(node);
            }
        }
    }
    out
}

type Auto = (String, bool, Option<Span>, Vec<String>);

fn collect_autos(nodes: &[Node], out: &mut Vec<Auto>) {
    for node in nodes {
        if let Node::Custom(c) = node
            && c.kind == "api-auto"
        {
            let scope: Vec<String> = c
                .attrs
                .get("scope")
                .and_then(|s| serde_json::from_value(s.clone()).ok())
                .unwrap_or_default();
            out.push((
                argument(c).to_owned(),
                !flag(c, "no-members"),
                c.span,
                scope,
            ));
            continue;
        }
        collect_autos(node.children(), out);
    }
}

fn replace_autos(nodes: Vec<Node>, rendered: &mut impl Iterator<Item = Vec<Node>>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    for mut node in nodes {
        if let Node::Custom(c) = &node
            && c.kind == "api-auto"
        {
            out.extend(rendered.next().unwrap_or_default());
            continue;
        }
        if let Some(children) = node.children_mut() {
            let replaced = replace_autos(std::mem::take(children), rendered);
            *children = replaced;
        }
        out.push(node);
    }
    out
}

/// Resolves references, tracking scope: an `api-scope` marker sets it for
/// what follows (and is removed), an `api-item` for its children.
fn resolve_scoped(
    language: &impl Language,
    index: &Index,
    nodes: &mut Vec<Node>,
    page: &str,
    scope: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut i = 0;
    while i < nodes.len() {
        if let Node::Custom(c) = &nodes[i]
            && c.kind == "api-scope"
        {
            *scope = c
                .attrs
                .get("scope")
                .and_then(|s| serde_json::from_value(s.clone()).ok())
                .unwrap_or_default();
            nodes.remove(i);
            continue;
        }
        let item_scope = match &nodes[i] {
            Node::Custom(c) if c.kind == "api-item" => c
                .attrs
                .get("scope")
                .and_then(|s| serde_json::from_value::<Vec<String>>(s.clone()).ok()),
            _ => None,
        };
        if let Some(replacement) = role(language, index, &nodes[i], page, scope, diagnostics) {
            let len = replacement.len();
            nodes.splice(i..=i, replacement);
            i += len;
            continue;
        }
        if let Some(children) = nodes[i].children_mut() {
            match item_scope {
                Some(mut inner) => {
                    resolve_scoped(language, index, children, page, &mut inner, diagnostics)
                }
                None => resolve_scoped(language, index, children, page, scope, diagnostics),
            }
        }
        i += 1;
    }
}

/// Resolves the domain's references in `nodes`, all in `scope`.
pub(crate) fn resolve_roles(
    language: &impl Language,
    index: &Index,
    nodes: &mut Vec<Node>,
    page: &str,
    scope: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut scope = scope.to_vec();
    resolve_scoped(language, index, nodes, page, &mut scope, diagnostics);
}

/// A domain reference's replacement, if `node` is one.
fn role(
    language: &impl Language,
    index: &Index,
    node: &Node,
    page: &str,
    scope: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<Node>> {
    let Node::Custom(c) = node else {
        return None;
    };
    if c.kind != "role" {
        return None;
    }
    let name = c.attrs.get("name")?.as_str()?;
    let role = name.strip_prefix(language.domain())?.strip_prefix(':')?;
    let kinds = language.roles().into_iter().find(|(r, _)| r == role)?.1;
    let raw = c
        .attrs
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (title, target) = split_title(raw);
    let (short, target) = match target.strip_prefix('~') {
        Some(rest) => (true, rest.to_owned()),
        None => (false, target),
    };
    let target = target.trim_start_matches('.').to_owned();
    let callable = !kinds.is_empty() && kinds.iter().all(ItemKind::is_callable);
    let shown = title.unwrap_or_else(|| {
        let mut shown = if short {
            split_name(&target).last().cloned().unwrap_or_default()
        } else {
            target.clone()
        };
        if callable && !shown.ends_with(')') {
            shown.push_str("()");
        }
        shown
    });
    let kinds = (!kinds.is_empty()).then_some(kinds);
    let found = match index.resolve(language.domain(), &target, scope, kinds.as_deref()) {
        Resolved::Found(entry) => Some(entry),
        Resolved::Ambiguous(entries) => {
            let names: Vec<String> = entries
                .iter()
                .map(|e| e.path.join(language.separator()))
                .collect();
            diagnostics.push(warning(
                c.span,
                format!("more than one target for {target:?}: {}", names.join(", ")),
            ));
            entries.first().copied()
        }
        Resolved::Missing => None,
    };
    let shown_node = match title_is_code(&shown) {
        true => code(&shown),
        false => Node::Text(Text {
            span: c.span,
            value: shown.clone(),
        }),
    };
    Some(match found {
        Some(entry) => {
            let mut node = link(page, &entry.page, Some(&entry.anchor), vec![shown_node]);
            *node.span_mut() = c.span;
            vec![node]
        }
        None => vec![shown_node],
    })
}

/// References show as code unless their text has spaces (a title).
fn title_is_code(shown: &str) -> bool {
    !shown.contains(' ')
}

/// `text <target>`: the text (if any) and the target.
fn split_title(text: &str) -> (Option<String>, String) {
    let text = text.trim();
    if let Some(rest) = text.strip_suffix('>')
        && let Some(i) = rest.rfind('<')
        && i > 0
        && rest[..i].ends_with(char::is_whitespace)
    {
        return (
            Some(rest[..i].trim().to_owned()),
            rest[i + 1..].trim().to_owned(),
        );
    }
    (None, text.to_owned())
}
