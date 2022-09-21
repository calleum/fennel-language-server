# fennel-language-server

[![Test](https://github.com/rydesun/fennel-language-server/actions/workflows/test.yaml/badge.svg)](https://github.com/rydesun/fennel-language-server/actions/workflows/test.yaml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/rydesun/fennel-language-server/blob/master/LICENSE)

Fennel language server protocol (LSP) support.

`fennel-language-server` is currently in a very early stage and unreliable.
Use it just for an encouraging try.

## Installation

Because it is written in pure Rust language,
the server should be installed via `cargo`.

```sh
cargo install --git https://github.com/rydesun/fennel-language-server
```

No demand for the Fennel environment. You don't even need Fennel runtime!
(It sounds a little weird but that's the truth)

## Integration

### Neovim

For Nvim user to setup `fennel-language-server` with `nvim-lspconfig`,
add the following code to your configuration.

```lua
local configs = require 'lspconfig.configs'
configs.fennel_language_server = {
  default_config = {
    -- replace it with true path
    cmd = {'/PATH/TO/BINFILE'},
    filetypes = {'fennel'},
    -- nowadays only support single file mode
    single_file_support = true,
    settings = {
      fennel = {
        diagnostics = {
          globals = {'vim'},
        },
      },
    },
  },
}
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
