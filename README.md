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

There is a long way to go.
Features are partially completed:

- [x] `Diagnostics`: Be careful these are not fully provided!
- [x] `Goto Definition`
- [x] `Code Completion`
- [x] `References`
- [x] `Hover`
- [x] `Rename`
- [x] `Code Action`
- [x] `Document Symbol`
- [ ] `Formatter`

**All features don't work properly on multi-symbols.**
It means that you cannot hover on the part after the dot, for example.

The following are also known issues:

- Macro grammar support is very limited.
  You may suffer from wrong diagnostics.
- Type checking is very weak.
- Lack of cross-file operation.
  Such as `require-macros` still does not analyzed.
  You should use `import-macros` for a clear namespace.

## Also See

XeroOl `fennel-ls` written in pure fennel you may love

[https://git.sr.ht/~xerool/fennel-ls](https://git.sr.ht/~xerool/fennel-ls)
