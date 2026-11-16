//! The lsdocs side of API documentation, for language packages to build
//! on.
//!
//! A language package implements [`Language`]: it extracts [`Item`]s from
//! source code and says how its signatures look and what its domain's
//! directives and roles are. [`plugin`] turns it into an lsdocs plugin (a
//! transformer) that:
//!
//! - **generates pages** (through `transform/collected`): one per module,
//!   under `output` (default `api/<domain>`), each item a section with its
//!   signature and its doc comment, plus an index page listing the
//!   modules. Doc comments are split into sections by
//!   [`lsdocs_apidoc::docstring`] and their text parsed by the site's
//!   parser for the language's doc markup (`site/parse`), so Markdown or
//!   reStructuredText in a docstring becomes real document nodes;
//! - **declares the domain's directives and roles** (syntax extensions):
//!   `.. <domain>:<object>:: signature` describes an item by hand
//!   (`py:function`, `cpp:class`), `.. <domain>:module::` sets the module
//!   the page is about, `.. <domain>:auto:: name` places a generated item
//!   in a hand-written page, and `` :<domain>:<role>:`name` `` links to an
//!   item, generated or described by hand (`~name` shows only the last
//!   part; `text <name>` gives the text);
//! - **resolves references** with an index of every item: names resolve
//!   in the scope they are written in, then as the end of one qualified
//!   name. Item anchors are `<domain>.<qualified name>`, so `ref` and
//!   links by anchor work too.
//!
//! Options every language package takes (besides its own, e.g. paths):
//!
//! | Option | Default | Meaning |
//! |---|---|---|
//! | `output` | `"api/<domain>"` | Where generated pages go, under the source directory. |
//! | `private` | `false` | Document private items too. |
//! | `title` | `"<Language> API"` | The title of the index page. |
//! | `doc_format` | the language's | The extension of the parser for doc comments (`md`, `rst`). |
//! | `doc_style` | `"auto"` | Docstring sections: `auto`, `plain`, `google`, `numpy`, `sphinx`, `tags`, `headings`. |
//! | `generate` | `true` | Generate pages (off: only the directives and roles). |
//! | `primary` | `false` | Also take the domain's roles without their prefix (`:func:` for `:py:func:`), as Sphinx's `primary_domain` does. |
//!
//! List the plugin before `transform-links`, `transform-toc` and
//! `transform-highlight`.

mod domain;
mod render;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

pub use lsdocs_apidoc;
use lsdocs_apidoc::index::Entry;
use lsdocs_apidoc::{DocStyle, Index, Item, ItemKind, Param, ParamKind, Signature, Visibility};
use lsdocs_plugin_sdk::doctree::Document;
use lsdocs_plugin_sdk::protocol::{
    ArgumentSyntax, ContentSyntax, Diagnostic, DirectiveSyntax, InitializeParams, OptionSyntax,
    RoleSyntax, Severity, Syntax, TransformApplyParams, TransformApplyResult,
    TransformCollectParams, TransformCollectResult, TransformCollectedResult,
};
use lsdocs_plugin_sdk::{Context, Plugin, PluginError, PluginResult, Transformer};
use serde::Deserialize;
use serde_json::{Value, json};

/// What a language package provides.
pub trait Language: Send + Sync + 'static {
    /// The domain name, in directives, roles and anchors: `py`, `rs`, `js`,
    /// `cpp`.
    fn domain(&self) -> &str;

    /// The language's name, for titles: `Python`.
    fn name(&self) -> &str;

    /// The language of signature code blocks, for highlighting: `python`.
    fn code_language(&self) -> &str;

    /// The separator of qualified names as the language writes them: `.`
    /// or `::`.
    fn separator(&self) -> &str {
        "."
    }

    /// The extension of the parser for doc comments: `md` or `rst`.
    fn doc_format(&self) -> &str {
        "md"
    }

    /// The docstring convention its comments usually follow.
    fn doc_style(&self) -> DocStyle {
        DocStyle::Auto
    }

    /// Extracts the API: the top-level items (usually modules), the files
    /// read, and problems found. `options` are the plugin's options;
    /// `root` is the site directory, which relative paths start from.
    fn extract(&self, options: &Value, root: &Path) -> Result<Extracted, String>;

    /// An item's signature, as shown above its documentation.
    fn signature(&self, item: &Item) -> String {
        default_signature(item, self.separator())
    }

    /// The domain's object directives (`function`, `class`, ...) and the
    /// kind of item each describes.
    fn objects(&self) -> Vec<(String, ItemKind)> {
        [
            ("function", ItemKind::Function),
            ("class", ItemKind::Class),
            ("method", ItemKind::Method),
            ("attribute", ItemKind::Field),
            ("property", ItemKind::Property),
            ("data", ItemKind::Constant),
            ("exception", ItemKind::Class),
            ("type", ItemKind::TypeAlias),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_owned(), k))
        .collect()
    }

    /// The domain's roles and the kinds each refers to (empty: any).
    fn roles(&self) -> Vec<(String, Vec<ItemKind>)> {
        use ItemKind::*;
        [
            ("func", vec![Function, Method, Constructor, Macro]),
            ("meth", vec![Method, Constructor, Function]),
            ("class", vec![Class, Struct, Interface, Trait, Enum, Union]),
            ("attr", vec![Field, Property, Constant, Variable, Variant]),
            ("data", vec![Constant, Variable]),
            ("mod", vec![Module, Package, Namespace]),
            (
                "type",
                vec![TypeAlias, Class, Struct, Interface, Trait, Enum, Union],
            ),
            ("obj", vec![]),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_owned(), k))
        .collect()
    }

    /// The item a hand-written object description stands for: `object` is
    /// the directive's name without the domain (`function`), `kind` its
    /// kind, `signature` its argument, `scope` the module (or type) it is
    /// in. `None` if the signature makes no sense.
    fn describe(&self, kind: ItemKind, signature: &str, scope: &[String]) -> Option<Item> {
        default_describe(kind, signature, scope, self.separator())
    }
}

/// What [`Language::extract`] found.
#[derive(Debug, Clone, Default)]
pub struct Extracted {
    /// Top-level items, usually modules.
    pub items: Vec<Item>,
    /// The files read, so the pages are made again when one changes.
    pub files: Vec<String>,
    /// Problems found.
    pub diagnostics: Vec<Diagnostic>,
}

/// The options every language package takes.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub(crate) struct Options {
    pub output: Option<String>,
    pub private: bool,
    pub title: Option<String>,
    pub doc_format: Option<String>,
    pub doc_style: Option<DocStyle>,
    pub generate: bool,
    pub primary: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            output: None,
            private: false,
            title: None,
            doc_format: None,
            doc_style: None,
            generate: true,
            primary: false,
        }
    }
}

impl Options {
    fn read(value: &Value) -> Result<Self, PluginError> {
        if value.is_null() {
            return Ok(Self::default());
        }
        Self::deserialize(value).map_err(|e| PluginError::new(format!("invalid options: {e}")))
    }
}

/// What the plugin knows after generating, for the apply pass.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub index: Index,
    /// Generated items by qualified name, for `auto`.
    pub items: BTreeMap<Vec<String>, Item>,
}

/// The transformer for a language.
pub(crate) struct Api<L: Language> {
    pub language: Arc<L>,
    pub state: Mutex<Arc<State>>,
}

/// The plugin for `language`, named `apidoc-<domain>`.
pub fn plugin<L: Language>(language: L, version: &str) -> Plugin {
    let language = Arc::new(language);
    let name = format!("apidoc-{}", language.domain());
    let for_syntax = Arc::clone(&language);
    Plugin::new(name, version)
        .transformer(Api {
            language,
            state: Mutex::default(),
        })
        .syntax(move |init: &InitializeParams| {
            let primary = Options::read(&init.options).is_ok_and(|o| o.primary);
            syntax(&*for_syntax, primary)
        })
        .cacheable()
}

/// The directives and roles of `language`'s domain.
pub(crate) fn syntax(language: &impl Language, primary: bool) -> Syntax {
    let d = language.domain();
    let mut directives = Vec::new();
    for (object, _) in language.objects() {
        directives.push(DirectiveSyntax {
            name: format!("{d}:{object}"),
            required_arguments: 1,
            final_argument_whitespace: true,
            options: [
                ("noindex", OptionSyntax::Flag),
                ("module", OptionSyntax::Required),
            ]
            .into_iter()
            .map(|(n, k)| (n.to_owned(), k))
            .collect(),
            content: ContentSyntax::Blocks,
            ..DirectiveSyntax::default()
        });
    }
    for module in ["module", "currentmodule"] {
        directives.push(DirectiveSyntax {
            name: format!("{d}:{module}"),
            required_arguments: 1,
            options: [
                ("noindex", OptionSyntax::Flag),
                ("synopsis", OptionSyntax::Value),
                ("platform", OptionSyntax::Value),
                ("deprecated", OptionSyntax::Flag),
            ]
            .into_iter()
            .map(|(n, k)| (n.to_owned(), k))
            .collect(),
            content: ContentSyntax::Blocks,
            ..DirectiveSyntax::default()
        });
    }
    directives.push(DirectiveSyntax {
        name: format!("{d}:auto"),
        required_arguments: 1,
        options: [("no-members", OptionSyntax::Flag)]
            .into_iter()
            .map(|(n, k)| (n.to_owned(), k))
            .collect(),
        argument: ArgumentSyntax::Text,
        ..DirectiveSyntax::default()
    });
    let mut roles: Vec<RoleSyntax> = language
        .roles()
        .into_iter()
        .map(|(name, _)| RoleSyntax {
            name: format!("{d}:{name}"),
        })
        .collect();
    if primary {
        roles.extend(
            language
                .roles()
                .into_iter()
                .map(|(name, _)| RoleSyntax { name }),
        );
    }
    Syntax { directives, roles }
}

/// A warning about API documentation.
pub(crate) fn warning(
    span: Option<lsdocs_plugin_sdk::doctree::Span>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        message: message.into(),
        span,
        code: Some("apidoc".into()),
        ..Diagnostic::default()
    }
}

impl<L: Language> Api<L> {
    fn output(&self, options: &Options) -> String {
        options
            .output
            .clone()
            .unwrap_or_else(|| format!("api/{}", self.language.domain()))
            .trim_matches('/')
            .to_owned()
    }
}

impl<L: Language> Transformer for Api<L> {
    const COLLECTS: bool = true;

    async fn collect(
        &self,
        params: TransformCollectParams,
        ctx: &Context,
    ) -> PluginResult<TransformCollectResult> {
        Options::read(ctx.options())?;
        let described = domain::described(&*self.language, &params.document);
        let items: Vec<Value> = described
            .into_iter()
            .map(|(path, kind)| json!({"path": path, "kind": kind}))
            .collect();
        Ok(TransformCollectResult {
            data: json!({ "items": items }),
            ..TransformCollectResult::default()
        })
    }

    async fn generate(
        &self,
        collected: &Value,
        ctx: &Context,
    ) -> PluginResult<TransformCollectedResult> {
        let options = Options::read(ctx.options())?;
        let language = &*self.language;
        let mut state = State::default();
        let mut result = TransformCollectedResult::default();
        // Items described by hand, where they are.
        for (page, data) in collected.as_object().into_iter().flatten() {
            for item in data["items"].as_array().into_iter().flatten() {
                let path: Vec<String> =
                    serde_json::from_value(item["path"].clone()).unwrap_or_default();
                let kind: ItemKind =
                    serde_json::from_value(item["kind"].clone()).unwrap_or(ItemKind::Function);
                state.index.add(Entry {
                    domain: language.domain().to_owned(),
                    anchor: lsdocs_apidoc::anchor(language.domain(), &path),
                    path,
                    kind,
                    page: page.clone(),
                });
            }
        }
        if options.generate {
            let root = ctx
                .site()
                .root
                .clone()
                .map(PathBuf::from)
                .unwrap_or_default();
            let extracted = language
                .extract(ctx.options(), &root)
                .map_err(|e| PluginError::new(format!("{}: {e}", language.name())))?;
            result.diagnostics.extend(extracted.diagnostics);
            result.dependencies = extracted.files;
            let mut items = extracted.items;
            if !options.private {
                items.retain(|i| i.visibility == Visibility::Public);
                for item in &mut items {
                    item.retain(&|i| i.visibility == Visibility::Public);
                }
            }
            let output = self.output(&options);
            let pages = render::pages(&items, &output);
            for (page, item) in &pages.placed {
                state.index.add(Entry {
                    domain: language.domain().to_owned(),
                    path: item.path.clone(),
                    kind: item.kind.clone(),
                    page: page.clone(),
                    anchor: lsdocs_apidoc::anchor(language.domain(), &item.path),
                });
            }
            for item in items.iter().flat_map(Item::walk) {
                state.items.insert(item.path.clone(), item.clone());
            }
            let renderer = render::Renderer::new(language, &options, ctx);
            result.documents = renderer.documents(&items, &output, &state.index).await;
            result.diagnostics.extend(renderer.take_diagnostics());
        }
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(state);
        Ok(result)
    }

    async fn apply(
        &self,
        params: TransformApplyParams,
        ctx: &Context,
    ) -> PluginResult<TransformApplyResult> {
        let options = Options::read(ctx.options())?;
        let state = Arc::clone(&self.state.lock().unwrap_or_else(PoisonError::into_inner));
        let mut document: Document = params.document;
        let mut diagnostics = Vec::new();
        let renderer = render::Renderer::new(&*self.language, &options, ctx);
        domain::apply(
            &*self.language,
            &state,
            &renderer,
            &mut document,
            &mut diagnostics,
        )
        .await;
        Ok(TransformApplyResult {
            document,
            diagnostics,
            ..TransformApplyResult::default()
        })
    }
}

/// A signature in a generic notation: `async name<T>(a: int = 1, *rest) ->
/// str` for callables, `class Name<T>(Base)` for types, `name: type =
/// value` for fields and constants.
pub fn default_signature(item: &Item, separator: &str) -> String {
    if let Some(text) = item.signature.as_ref().and_then(|s| s.text.clone()) {
        return text;
    }
    let name = item.name();
    let generics = item
        .signature
        .as_ref()
        .filter(|s| !s.generics.is_empty())
        .map(|s| format!("<{}>", s.generics.join(", ")))
        .unwrap_or_default();
    let typed = |ty: &Option<String>, value: &Option<String>| {
        let mut out = name.to_owned();
        if let Some(ty) = ty {
            out.push_str(": ");
            out.push_str(ty);
        }
        if let Some(value) = value {
            out.push_str(" = ");
            out.push_str(value);
        }
        out
    };
    match &item.kind {
        kind if kind.is_module() => format!("{} {}", kind.label(), item.qualified(separator)),
        kind if kind.is_callable() => {
            let sig = item.signature.clone().unwrap_or_default();
            let mut out = String::new();
            for modifier in &sig.modifiers {
                out.push_str(modifier);
                out.push(' ');
            }
            out.push_str(name);
            out.push_str(&generics);
            out.push('(');
            let params: Vec<String> = sig.params.iter().map(param).collect();
            out.push_str(&params.join(", "));
            out.push(')');
            if let Some(returns) = &sig.returns {
                out.push_str(" -> ");
                out.push_str(returns);
            }
            out
        }
        ItemKind::TypeAlias => match &item.ty {
            Some(ty) => format!("type {name}{generics} = {ty}"),
            None => format!("type {name}{generics}"),
        },
        kind if kind.is_type() => {
            let mut out = format!("{} {name}{generics}", kind.label());
            if !item.bases.is_empty() {
                out.push_str(&format!("({})", item.bases.join(", ")));
            }
            out
        }
        _ => typed(&item.ty, &item.value),
    }
}

fn param(p: &Param) -> String {
    let prefix = match p.kind {
        ParamKind::Variadic => "*",
        ParamKind::KeywordVariadic => "**",
        _ => "",
    };
    let mut out = format!("{prefix}{}", p.name);
    if let Some(ty) = &p.ty {
        out.push_str(": ");
        out.push_str(ty);
    }
    if let Some(default) = &p.default {
        out.push_str(" = ");
        out.push_str(default);
    }
    out
}

/// Reads a signature in the generic notation [`default_signature`]
/// writes: `name(a: int = 1, *rest) -> str`, or for other kinds `name`,
/// `name: type = value`, `name(Base)`.
pub fn default_describe(
    kind: ItemKind,
    signature: &str,
    scope: &[String],
    separator: &str,
) -> Option<Item> {
    let signature = signature.trim();
    // The name ends at what follows it; `::` is part of it.
    let bytes = signature.as_bytes();
    let mut name_end = signature.len();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b':' if bytes.get(i + 1) == Some(&b':') => i += 2,
            b'(' | b'<' | b':' | b'=' | b' ' => {
                name_end = i;
                break;
            }
            _ => i += 1,
        }
    }
    let qualified = &signature[..name_end];
    if qualified.is_empty() {
        return None;
    }
    let mut path = scope.to_vec();
    path.extend(
        qualified
            .split(separator)
            .flat_map(|p| p.split('.'))
            .filter(|p| !p.is_empty())
            .map(str::to_owned),
    );
    let mut item = Item::new(kind.clone(), path);
    let rest = signature[name_end..].trim();
    let (generics, rest) = match rest.strip_prefix('<') {
        Some(after) => match matching(after, '<', '>') {
            Some(end) => (split_top(&after[..end]), after[end + 1..].trim()),
            None => (Vec::new(), rest),
        },
        None => (Vec::new(), rest),
    };
    if let Some(after) = rest.strip_prefix('(') {
        let end = matching(after, '(', ')')?;
        let inside = &after[..end];
        let tail = after[end + 1..].trim();
        if kind.is_callable() {
            let params = split_top(inside)
                .into_iter()
                .filter(|p| !p.is_empty() && p != "/" && p != "*")
                .map(|p| parse_param(&p))
                .collect();
            let returns = tail
                .strip_prefix("->")
                .or_else(|| tail.strip_prefix(':'))
                .map(|r| r.trim().to_owned())
                .filter(|r| !r.is_empty());
            item.signature = Some(Signature {
                generics,
                params,
                returns,
                ..Signature::default()
            });
        } else {
            item.bases = split_top(inside)
                .into_iter()
                .filter(|b| !b.is_empty())
                .collect();
            if !generics.is_empty() {
                item.signature = Some(Signature {
                    generics,
                    ..Signature::default()
                });
            }
        }
    } else {
        if !generics.is_empty() {
            item.signature = Some(Signature {
                generics,
                ..Signature::default()
            });
        }
        let (ty, value) = match rest.split_once('=') {
            Some((ty, value)) => (ty, Some(value.trim().to_owned())),
            None => (rest, None),
        };
        let ty = ty.trim().trim_start_matches(':').trim();
        item.ty = (!ty.is_empty()).then(|| ty.to_owned());
        item.value = value.filter(|v| !v.is_empty());
        if kind.is_callable() {
            item.signature.get_or_insert_with(Signature::default);
        }
    }
    Some(item)
}

/// `name: type = default`, with `*` and `**` for variadics.
fn parse_param(text: &str) -> Param {
    let text = text.trim();
    let (kind, text) = if let Some(rest) = text.strip_prefix("**") {
        (ParamKind::KeywordVariadic, rest)
    } else if let Some(rest) = text.strip_prefix('*') {
        (ParamKind::Variadic, rest)
    } else if let Some(rest) = text.strip_prefix("...") {
        (ParamKind::Variadic, rest)
    } else {
        (ParamKind::Normal, text)
    };
    let (head, default) = match text.split_once('=') {
        Some((head, default)) => (head, Some(default.trim().to_owned())),
        None => (text, None),
    };
    let (name, ty) = match head.split_once(':') {
        Some((name, ty)) => (name.trim(), Some(ty.trim().to_owned())),
        None => (head.trim(), None),
    };
    let kind = if matches!(name, "self" | "this" | "&self" | "&mut self" | "cls") {
        ParamKind::Receiver
    } else {
        kind
    };
    Param {
        name: name.to_owned(),
        ty: ty.filter(|t| !t.is_empty()),
        default,
        kind,
    }
}

/// The index in `text` of the bracket closing an `open` already passed.
fn matching(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// `text` split at commas outside brackets and quotes, each part trimmed.
fn split_top(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut current = String::new();
    let mut previous = ' ';
    for c in text.chars() {
        let arrow = c == '>' && previous == '-';
        previous = c;
        match (quote, c) {
            (None, '>') if arrow => {}
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[' | '{' | '<') => depth += 1,
            (None, ')' | ']' | '}' | '>') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }
    parts
}
