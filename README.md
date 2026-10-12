# fennel-language-server

[![Test](https://github.com/calleum/fennel-language-server/actions/workflows/test.yaml/badge.svg)](https://github.com/calleum/fennel-language-server/actions/workflows/test.yaml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/calleum/fennel-language-server/blob/main/LICENSE)

Fennel language server protocol (LSP) support.

This is a maintained fork of [rydesun/fennel-language-server](https://github.com/rydesun/fennel-language-server).

`fennel-language-server` is currently in an early stage. Contributions are welcome!

## Installation

### Via Cargo

You can also install it via `cargo`.

```sh
cargo install --git https://github.com/calleum/fennel-language-server
```

No demand for the Fennel environment. You don't even need Fennel runtime!

## Integration

**NOTE**: The executable file is named `fennel-language-server`.

### Neovim

Neovim 0.11+ provides a built-in way to configure and enable language servers using `vim.lsp.config` and `vim.lsp.enable`.

```lua
vim.lsp.config('fennel_language_server', {
  cmd = { 'fennel-language-server' },
  filetypes = { 'fennel' },
  root_markers = { '.nfnl.fnl', 'fnl', '.git' },
  settings = {
    fennel = {
      workspace = {
        library = vim.api.nvim_list_runtime_paths(),
      },
      diagnostics = {
        globals = { 'vim' },
      },
    },
  },
})

vim.lsp.enable('fennel_language_server')
```

If you prefer `nvim-lspconfig`:

```lua
require('lspconfig').fennel_language_server.setup({
  settings = {
    fennel = {
      workspace = {
        library = vim.api.nvim_list_runtime_paths(),
      },
      diagnostics = {
        globals = { 'vim' },
      },
    },
  },
})
```

## Status

Most of the LSP surface is implemented. Diagnostics are the weakest area —
they are syntax- and name-based, not type-based.

Navigation

- [x] `Goto Definition` — including cross-file and field/method access
- [x] `References`
- [x] `Call Hierarchy` — prepare, incoming, outgoing
- [x] `Document Link` — `require` and `include` paths

Editing

- [x] `Rename`, with `prepare_rename` and cross-file edits
- [x] `Code Action` — diagnostic quick fixes and refactors
- [x] `Document Highlight`
- [x] `Inlay Hints` — parameter names at call sites
- [x] `Formatter` — see the caveats below

Symbols and structure

- [x] `Document Symbol` / `Workspace Symbol`
- [x] `Folding Range`
- [x] `Selection Range`
- [x] `Semantic Tokens`

Language intelligence

- [x] `Hover` — including the part after a `.` or `:` accessor
- [x] `Code Completion`, with auto-import for other modules
- [x] `Signature Help`
- [x] `Macro expansion inspection`

Infrastructure

- [x] `Diagnostics`
- [x] Background workspace indexing
- [x] File-watcher support, so edits made outside the editor are picked up

### About the formatter

The formatter is CST-based, so comments and string contents survive. It is
conservative by design:

- If the document does not parse cleanly, formatting is **declined** — no edits
  are returned. Rewriting code the server does not understand can turn one typo
  into another.
- Formatting is **idempotent**: re-formatting formatted code is a no-op.
- Forms that fit within 72 columns stay on one line; longer ones break, with
  the closing delimiter dedented to the opening delimiter's level.
- Range formatting is not supported. Indentation is only meaningful relative to
  the enclosing form, so formatting a fragment in isolation is declined rather
  than guessed at.

### Known issues

- **Macro grammar support is limited.** The macro expander covers the common
  threading and collection macros, but unusual macro bodies can still produce
  wrong diagnostics.
- **Type checking is very weak.** The server resolves names and signatures; it
  does not infer or check types.
- **`require-macros` is deprecated.** Use `import-macros` for a clear
  namespace — the server reports `require-macros` as deprecated.
- **No formatter upstream.** Upstream Fennel does not ship one either, so this
  formatter is home-grown rather than delegated to an external tool. It is not
  yet configurable and does not honour the usual editor formatting options
  (tab width, etc.).

## Contributing

The parser has a robustness harness that runs on every `cargo test`. It checks
that the parser never panics, never hangs, and never reports an error range
outside the source text:

```sh
cargo test -p fennel-parser --test robustness
```

For a longer randomized search (reproducible by seed):

```sh
cargo run --release -p fennel-parser --example fuzz -- 200000
```

## Also See

XeroOl `fennel-ls` written in pure fennel you may love

[https://git.sr.ht/~xerool/fennel-ls](https://git.sr.ht/~xerool/fennel-ls)
