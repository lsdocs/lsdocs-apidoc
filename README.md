# lsdocs-apidoc

Part of [lsdocs](https://github.com/lsdocs), a static-site generator whose
parsers, transformers and renderers are plugins.

The API documentation framework:

| Path | What |
|---|---|
| `crates/apidoc` | `lsdocs-apidoc`: a standalone model of API documentation (items, signatures, docstring sections and styles, name resolution). Depends on nothing of lsdocs; keep it that way. |
| `crates/apidoc-plugin` | `lsdocs-apidoc-plugin`: the lsdocs side. A language package implements its `Language` trait; it generates pages, declares the domain's directives and roles, and resolves references. |

The language packages are lsdocs-apidoc-python, -typescript, -rust and
-clang.

The plugin's settings and output are described in the lsdocs
documentation (https://lsdocs.github.io, "API documentation").

## Known limitations

- No extractor for the domains `rst` (`rst:directive`, `rst:role`) and
  `std`'s `option` / `program`, `envvar` and `productionlist` / `token`
  (`py`, `c`, `cpp`, `js`, `rs` come with their language packages).
- One page per module; no page per item.

## TODO

- [ ] Extractors (or directives only) for the `rst` and remaining `std`
      domains.
- [ ] A page per item, for large APIs (the user's choice).
