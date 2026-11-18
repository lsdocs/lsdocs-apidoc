#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use lsdocs_apidoc::{Item, ItemKind};
use lsdocs_plugin_sdk::doctree::{Document, Node, plain_text};
use lsdocs_plugin_sdk::protocol::{Diagnostic, InitializeParams, SiteInfo};
use lsdocs_plugin_sdk::testing::{context_with_parsers, run_transform_in};
use serde_json::{Value, json};

use super::{Extracted, Language, default_describe, default_signature, plugin, syntax};

/// A language whose items are the plugin's `items` option: the framework
/// tested without an extractor.
struct Fake;

impl Language for Fake {
    fn domain(&self) -> &str {
        "fk"
    }
    fn name(&self) -> &str {
        "Fake"
    }
    fn code_language(&self) -> &str {
        "text"
    }
    fn extract(&self, options: &Value, _root: &Path) -> Result<Extracted, String> {
        let items: Vec<Item> =
            serde_json::from_value(options["items"].clone()).map_err(|e| e.to_string())?;
        Ok(Extracted {
            items,
            files: vec!["src/pkg.fk".into()],
            diagnostics: Vec::new(),
        })
    }
}

fn items() -> Value {
    json!([{
        "kind": "module",
        "path": ["pkg"],
        "docs": "The **package**.",
        "members": [
            {
                "kind": "class",
                "path": ["pkg", "Greeter"],
                "bases": ["Base"],
                "docs": "Greets people.",
                "members": [{
                    "kind": "method",
                    "path": ["pkg", "Greeter", "greet"],
                    "signature": {
                        "params": [
                            {"name": "self", "kind": "receiver"},
                            {"name": "name", "ty": "str", "default": "\"you\""}
                        ],
                        "returns": "str"
                    },
                    "docs": "Say hello.\n\nArgs:\n    name: Who, with {fk:func}`helper`.\n\nReturns:\n    str: The greeting.\n"
                }]
            },
            {
                "kind": "function",
                "path": ["pkg", "helper"],
                "signature": {"params": [{"name": "x", "ty": "int"}]},
                "docs": "Helps."
            },
            {
                "kind": "function",
                "path": ["pkg", "_hidden"],
                "visibility": "private"
            },
            {
                "kind": "module",
                "path": ["pkg", "sub"],
                "members": [{"kind": "constant", "path": ["pkg", "sub", "LIMIT"], "ty": "int", "value": "3"}]
            }
        ]
    }])
}

/// Parses `pages` with the plugin's syntax and runs the plugin over them,
/// with the Markdown and reStructuredText parsers for `host/parse`.
async fn build(pages: &[(&str, &str)], options: Value) -> Vec<(Document, Vec<Diagnostic>)> {
    let primary = options["primary"] == true;
    let syntax = syntax(&Fake, primary);
    let documents = pages
        .iter()
        .map(|(source, text)| {
            let (doc, diagnostics) = if source.ends_with(".rst") {
                lsdocs_parser_rst::parse(source, text, None, &syntax)
            } else {
                lsdocs_parser_markdown::parse_with_syntax(source, text, &syntax)
            };
            assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
            doc
        })
        .collect();
    let init = lsdocs_plugin_sdk::Setup::with_settings(
        InitializeParams {
            site: SiteInfo::default(),
            ..InitializeParams::default()
        },
        options,
    );
    let plugin = plugin(Fake, "0.0.0");
    let ctx = context_with_parsers(
        &plugin,
        &init,
        vec![
            lsdocs_parser_markdown::plugin(),
            lsdocs_parser_rst::plugin(),
        ],
    );
    run_transform_in(&plugin, &ctx, documents).await.unwrap()
}

fn find<'a>(doc: &'a Document, id: &str) -> &'a Node {
    doc.descendants()
        .find(|n| match n {
            Node::Section(s) => s.id.as_deref() == Some(id),
            Node::Anchor(a) => a.id == id,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no {id} in {doc:#?}"))
}

fn codes(node: &Node) -> Vec<String> {
    node.descendants()
        .filter_map(|n| match n {
            Node::CodeBlock(c) => Some(c.value.clone()),
            _ => None,
        })
        .collect()
}

fn links(node: &[Node]) -> Vec<String> {
    node.iter()
        .flat_map(Node::descendants)
        .filter_map(|n| match n {
            Node::Link(l) => Some(format!("{} -> {}", plain_text(&l.children), l.url)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn generates_a_page_per_module() {
    let out = build(&[], json!({"items": items()})).await;
    let sources: Vec<&str> = out.iter().map(|(d, _)| d.source.as_str()).collect();
    assert_eq!(
        sources,
        ["api/fk/index.md", "api/fk/pkg.md", "api/fk/pkg.sub.md"]
    );
    for (doc, diagnostics) in &out {
        assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", doc.source);
    }
    let (index, _) = &out[0];
    assert_eq!(index.metadata["title"], json!("Fake API"));
    assert_eq!(
        links(&index.children),
        ["pkg -> pkg.md", "pkg.sub -> pkg.sub.md"]
    );

    let (pkg, _) = &out[1];
    assert_eq!(pkg.title().as_deref(), Some("pkg"));
    // The module's doc comment, parsed as Markdown.
    assert!(
        pkg.descendants()
            .any(|n| matches!(n, Node::Strong(s) if plain_text(&s.children) == "package"))
    );
    let greeter = find(pkg, "fk.pkg.Greeter");
    assert_eq!(codes(greeter)[0], "class Greeter(Base)");
    let greet = find(pkg, "fk.pkg.Greeter.greet");
    assert_eq!(codes(greet), ["greet(self, name: str = \"you\") -> str"]);
    let text = plain_text(greet.children());
    assert!(
        text.contains("Parameters") && text.contains("Who, with"),
        "{text}"
    );
    assert!(
        text.contains("Returns") && text.contains("The greeting."),
        "{text}"
    );
    // The reference in the docstring resolves from the method's scope.
    assert_eq!(links(greet.children()), ["helper() -> #fk.pkg.helper"]);
    // Private items are left out; submodules are linked.
    assert!(
        !pkg.descendants()
            .any(|n| matches!(n, Node::Section(s) if s.id.as_deref() == Some("fk.pkg._hidden")))
    );
    assert!(links(&pkg.children).contains(&"pkg.sub -> pkg.sub.md".to_owned()));
    let (sub, _) = &out[2];
    assert_eq!(codes(find(sub, "fk.pkg.sub.LIMIT")), ["LIMIT: int = 3"]);

    // With `private`, the hidden function is there too.
    let out = build(&[], json!({"items": items(), "private": true})).await;
    find(&out[1].0, "fk.pkg._hidden");
}

#[tokio::test]
async fn hand_written_descriptions_auto_and_references() {
    let rst = ".. fk:module:: pkg\n\n\
               .. fk:function:: local(x: int) -> int\n\n   \
               Does *things*. See :fk:func:`helper` and :fk:class:`~pkg.Greeter`.\n\n\
               .. fk:auto:: helper\n\n\
               Also :fk:func:`local`, :fk:obj:`the greeter <Greeter>` and :fk:func:`missing`.\n";
    let md = "```{fk:currentmodule} pkg\n```\n\n\
              ```{fk:class} Other(Greeter)\nSee {fk:meth}`greet`.\n```\n";
    let out = build(
        &[("guide/a.rst", rst), ("b.md", md)],
        json!({"items": items()}),
    )
    .await;
    let (a, diagnostics) = out.iter().find(|(d, _)| d.source == "guide/a.rst").unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    find(a, "fk.pkg.local");
    let all_codes: Vec<String> = a.children.iter().flat_map(codes).collect();
    assert_eq!(all_codes, ["local(x: int) -> int", "helper(x: int)"]);
    assert_eq!(
        links(&a.children),
        [
            "helper() -> ../api/fk/pkg.md#fk.pkg.helper",
            "Greeter -> ../api/fk/pkg.md#fk.pkg.Greeter",
            "local() -> #fk.pkg.local",
            "the greeter -> ../api/fk/pkg.md#fk.pkg.Greeter",
        ]
    );
    // Not found: shown as code, no warning (as Sphinx without nitpicky).
    assert!(
        a.descendants()
            .any(|n| matches!(n, Node::InlineCode(c) if c.value == "missing()"))
    );
    // `currentmodule` makes no anchor; the class is described in it, and
    // its content resolves from the class.
    let (b, diagnostics) = out.iter().find(|(d, _)| d.source == "b.md").unwrap();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        !b.descendants()
            .any(|n| matches!(n, Node::Anchor(x) if x.id == "fk.pkg"))
    );
    find(b, "fk.pkg.Other");
    assert_eq!(
        links(&b.children),
        ["greet() -> api/fk/pkg.md#fk.pkg.Greeter.greet"]
    );
    // Hand-written items can be referenced from generated pages too: the
    // index has them.
    let a_md = "{fk:func}`pkg.local`\n";
    let out = build(
        &[("guide/a.rst", rst), ("c.md", a_md)],
        json!({"items": items()}),
    )
    .await;
    let (c, _) = out.iter().find(|(d, _)| d.source == "c.md").unwrap();
    assert_eq!(
        links(&c.children),
        ["pkg.local() -> guide/a.rst#fk.pkg.local"]
    );
}

#[tokio::test]
async fn problems_are_warnings() {
    let out = build(
        &[(
            "a.md",
            "```{fk:auto} nothing\n```\n\n```{fk:function} (x)\n```\n",
        )],
        json!({"items": items(), "generate": true}),
    )
    .await;
    let (_, diagnostics) = out.iter().find(|(d, _)| d.source == "a.md").unwrap();
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "fk:function: cannot read the signature \"(x)\"",
            "fk:auto: no item named \"nothing\""
        ]
    );
    // Without generated pages, only hand-written items.
    let out = build(&[("a.md", "x\n")], json!({"generate": false})).await;
    assert_eq!(out.len(), 1);
}

#[test]
fn signatures_round_trip() {
    let item = default_describe(
        ItemKind::Function,
        "mod.run<T>(self, x: Callable[[int], str] = f, *rest, **kw) -> list[T]",
        &["pkg".to_owned()],
        ".",
    )
    .unwrap();
    assert_eq!(item.path, ["pkg", "mod", "run"]);
    assert_eq!(
        default_signature(&item, "."),
        "run<T>(self, x: Callable[[int], str] = f, *rest, **kw) -> list[T]"
    );
    let field = default_describe(ItemKind::Constant, "LIMIT: int = 3", &[], ".").unwrap();
    assert_eq!(default_signature(&field, "."), "LIMIT: int = 3");
    let class = default_describe(ItemKind::Class, "Map<K, V>(Base, Other)", &[], ".").unwrap();
    assert_eq!(
        default_signature(&class, "."),
        "class Map<K, V>(Base, Other)"
    );
    let rust = default_describe(ItemKind::Function, "a::b(f: fn() -> u8)", &[], "::").unwrap();
    assert_eq!(rust.path, ["a", "b"]);
    assert_eq!(
        rust.signature.unwrap().params[0].ty.as_deref(),
        Some("fn() -> u8")
    );
    assert!(default_describe(ItemKind::Function, "(x)", &[], ".").is_none());
}

#[tokio::test]
async fn primary_domain_roles_have_no_prefix() {
    let out = build(
        &[("a.rst", "See :func:`pkg.helper`.\n")],
        json!({"items": items(), "primary": true}),
    )
    .await;
    let (a, _) = out.iter().find(|(d, _)| d.source == "a.rst").unwrap();
    assert_eq!(
        links(&a.children),
        ["pkg.helper() -> api/fk/pkg.md#fk.pkg.helper"]
    );
}

#[test]
fn unknown_settings_are_errors() {
    let dir = std::env::temp_dir().join(format!("lsdocs-apidoc-keys-{}", std::process::id()));
    let settings = dir.join("lsdocs/fk/settings.toml");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "output = \"api\"\nouptut = \"x\"\n").unwrap();
    let params = InitializeParams {
        config: lsdocs_plugin_sdk::protocol::ConfigInfo {
            dir: dir.to_string_lossy().into_owned(),
            name: "fk".into(),
        },
        ..InitializeParams::default()
    };
    let err = plugin(Fake, "0.0.0").setup(&params).unwrap_err();
    std::fs::write(&settings, "output = \"api\"\n").unwrap();
    let ok = plugin(Fake, "0.0.0").setup(&params);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(err.contains("`ouptut`"), "{err}");
    assert!(ok.is_ok(), "{ok:?}");
}
