# fennel-language-server

[![Test](https://github.com/rydesun/fennel-language-server/actions/workflows/test.yaml/badge.svg)](https://github.com/rydesun/fennel-language-server/actions/workflows/test.yaml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/rydesun/fennel-language-server/blob/master/LICENSE)

Fennel language server protocol (LSP) support.

`fennel-language-server` is currently in a very early stage and unreliable.
Use it just for an encouraging try.

## Installation

### Via Mason (Recommended for Neovim)

If you are using Neovim, the easiest way to install `fennel-language-server` is via [mason.nvim](https://github.com/williamboman/mason.nvim).

```vim
:MasonInstall fennel-language-server
```

### Via Cargo

Because it is written in Rust, you can also install it via `cargo`.

```sh
cargo install --git https://github.com/rydesun/fennel-language-server
```

No demand for the Fennel environment. You don't even need Fennel runtime!

## Integration

**NOTE**: The executable file is named `fennel-language-server`.
The former name `fennel-ls` has been abandoned (and now refers to a different implementation).

### Neovim

`fennel-language-server` is natively supported by [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig).

#### Basic Setup

```lua
require('lspconfig').fennel_language_server.setup({
  settings = {
    fennel = {
      workspace = {
        -- If you are using hotpot.nvim or aniseed,
        -- make the server aware of neovim runtime files.
        library = vim.api.nvim_list_runtime_paths(),
      },
      diagnostics = {
        globals = {'vim'},
      },
    },
  },
})
```

#### nfnl Setup

If you are using [nfnl](https://github.com/Olical/nfnl), use the following configuration:

```lua
require('lspconfig').fennel_language_server.setup({
  -- Ensure the LSP starts for nfnl projects
  root_dir = require('lspconfig').util.root_pattern(".nfnl.fnl", "fnl", ".git"),
  settings = {
    fennel = {
      workspace = {
        library = vim.api.nvim_get_runtime_file("", true),
      },
      diagnostics = {
        globals = {"vim"},
      },
    },
  },
})
```

*Note: If you have installed the server via Mason, ensure you have [mason-lspconfig.nvim](https://github.com/williamboman/mason-lspconfig.nvim) installed to automatically set up the path.*

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
