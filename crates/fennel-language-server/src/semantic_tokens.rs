use fennel_parser::{Ast, SyntaxKind, SyntaxNode, models::ScopeKind};
use ropey::Rope;
use tower_lsp::lsp_types::*;

pub const TOKEN_TYPE_FUNCTION: u32 = 0;
pub const TOKEN_TYPE_VARIABLE: u32 = 1;
pub const TOKEN_TYPE_MACRO: u32 = 2;
pub const TOKEN_TYPE_PARAMETER: u32 = 3;
pub const TOKEN_TYPE_KEYWORD: u32 = 4;
pub const TOKEN_TYPE_STRING: u32 = 5;
pub const TOKEN_TYPE_NUMBER: u32 = 6;
pub const TOKEN_TYPE_COMMENT: u32 = 7;
pub const TOKEN_TYPE_OPERATOR: u32 = 8;

pub const MODIFIER_DECLARATION: u32 = 1 << 0;
pub const MODIFIER_DEFAULT_LIBRARY: u32 = 1 << 1;

pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::FUNCTION,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::MACRO,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::OPERATOR,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    }
}

fn is_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::KEYWORD_FN
            | SyntaxKind::KEYWORD_LAMBDA
            | SyntaxKind::KEYWORD_LOCAL
            | SyntaxKind::KEYWORD_LET
            | SyntaxKind::KEYWORD_SET
            | SyntaxKind::KEYWORD_INCLUDE
            | SyntaxKind::KEYWORD_ACCUMULATE
            | SyntaxKind::KEYWORD_CATCH
            | SyntaxKind::KEYWORD_COLLECT
            | SyntaxKind::KEYWORD_DO
            | SyntaxKind::KEYWORD_DOTO
            | SyntaxKind::KEYWORD_EACH
            | SyntaxKind::KEYWORD_EVAL_COMPILER
            | SyntaxKind::KEYWORD_FOR
            | SyntaxKind::KEYWORD_GLOBAL
            | SyntaxKind::KEYWORD_ICOLLECT
            | SyntaxKind::KEYWORD_FCOLLECT
            | SyntaxKind::KEYWORD_IF
            | SyntaxKind::KEYWORD_IMPORT_MACROS
            | SyntaxKind::KEYWORD_INTO
            | SyntaxKind::KEYWORD_LUA
            | SyntaxKind::KEYWORD_MACRO
            | SyntaxKind::KEYWORD_MACRODEBUG
            | SyntaxKind::KEYWORD_MACROS
            | SyntaxKind::KEYWORD_MATCH
            | SyntaxKind::KEYWORD_MATCH_TRY
            | SyntaxKind::KEYWORD_PARTIAL
            | SyntaxKind::KEYWORD_PICK_ARGS
            | SyntaxKind::KEYWORD_VALUES
            | SyntaxKind::KEYWORD_PICK_VALUES
            | SyntaxKind::KEYWORD_REQUIRE_MACROS
            | SyntaxKind::KEYWORD_TSET
            | SyntaxKind::KEYWORD_UNTIL
            | SyntaxKind::KEYWORD_VAR
            | SyntaxKind::KEYWORD_WHEN
            | SyntaxKind::KEYWORD_WHERE
            | SyntaxKind::KEYWORD_WHILE
            | SyntaxKind::KEYWORD_WITH_OPEN
            | SyntaxKind::BOOL
            | SyntaxKind::NIL
    )
}

fn is_operator(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::OPERATOR | SyntaxKind::KEYWORD_OR | SyntaxKind::LENGTH | SyntaxKind::THREAD
    )
}

fn classify_symbol(token: &fennel_parser::SyntaxToken, ast: &Ast, std_ast: &Ast) -> (u32, u32) {
    let token_offset: u32 = token.text_range().start().into();
    let text = token.text();

    if let Some(lsym) = ast.l_symbol(token_offset) {
        let modifier = MODIFIER_DECLARATION;
        return match lsym.scope.kind {
            ScopeKind::Func => (TOKEN_TYPE_FUNCTION, modifier),
            ScopeKind::Macro => (TOKEN_TYPE_MACRO, modifier),
            ScopeKind::Param | ScopeKind::MacroParam => (TOKEN_TYPE_PARAMETER, modifier),
            ScopeKind::Global => (TOKEN_TYPE_VARIABLE, modifier | MODIFIER_DEFAULT_LIBRARY),
            _ => (TOKEN_TYPE_VARIABLE, modifier),
        };
    }

    if let Some(def) = ast.definition(token_offset) {
        match def {
            fennel_parser::Definition::Symbol(s, _)
            | fennel_parser::Definition::SymbolField(s, _) => {
                return match s.scope.kind {
                    ScopeKind::Func => (TOKEN_TYPE_FUNCTION, 0),
                    ScopeKind::Macro => (TOKEN_TYPE_MACRO, 0),
                    ScopeKind::Param | ScopeKind::MacroParam => (TOKEN_TYPE_PARAMETER, 0),
                    _ => (TOKEN_TYPE_VARIABLE, 0),
                };
            }
            fennel_parser::Definition::File(_) | fennel_parser::Definition::FileSymbol(_, _) => {
                return (TOKEN_TYPE_VARIABLE, 0);
            }
        }
    }

    let (std_symbols, _, _, _) = std_ast.completion(std_ast.end_offset(), None);
    if let Some(std_sym) = std_symbols.into_iter().find(|s| s.token.text == text) {
        return match std_sym.scope.kind {
            ScopeKind::Func => (TOKEN_TYPE_FUNCTION, MODIFIER_DEFAULT_LIBRARY),
            ScopeKind::Macro => (TOKEN_TYPE_MACRO, MODIFIER_DEFAULT_LIBRARY),
            _ => (TOKEN_TYPE_VARIABLE, MODIFIER_DEFAULT_LIBRARY),
        };
    }

    if ast.globals.contains(text) {
        return (TOKEN_TYPE_VARIABLE, MODIFIER_DEFAULT_LIBRARY);
    }

    let is_callee = token.parent().is_some_and(|p| p.kind() == SyntaxKind::N_SYMBOL_CALL);
    if is_callee { (TOKEN_TYPE_FUNCTION, 0) } else { (TOKEN_TYPE_VARIABLE, 0) }
}

pub fn semantic_tokens_full(doc: &Rope, ast: &Ast, std_ast: &Ast) -> SemanticTokens {
    let root = SyntaxNode::new_root(ast.root.clone());
    let mut raw_tokens = Vec::new();

    for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        let kind = token.kind();
        let classification = if kind == SyntaxKind::COMMENT {
            Some((TOKEN_TYPE_COMMENT, 0))
        } else if matches!(kind, SyntaxKind::QUOTE_STRING | SyntaxKind::COLON_STRING) {
            Some((TOKEN_TYPE_STRING, 0))
        } else if matches!(kind, SyntaxKind::FLOAT | SyntaxKind::INTEGER) {
            Some((TOKEN_TYPE_NUMBER, 0))
        } else if is_operator(kind) {
            Some((TOKEN_TYPE_OPERATOR, 0))
        } else if is_keyword(kind) {
            Some((TOKEN_TYPE_KEYWORD, 0))
        } else if matches!(kind, SyntaxKind::CAPTURE | SyntaxKind::VARARG) {
            Some((TOKEN_TYPE_PARAMETER, 0))
        } else if matches!(
            kind,
            SyntaxKind::SYMBOL | SyntaxKind::SYMBOL_FIELD | SyntaxKind::SYMBOL_METHOD
        ) {
            Some(classify_symbol(&token, ast, std_ast))
        } else {
            None
        };

        let Some((token_type, modifiers)) = classification else {
            continue;
        };

        let range = token.text_range();
        let start_byte: usize = range.start().into();
        let end_byte: usize = range.end().into();
        if start_byte >= end_byte || start_byte >= doc.len_bytes() {
            continue;
        }
        let end_byte = end_byte.min(doc.len_bytes());

        let start_char = doc.byte_to_char(start_byte);
        let end_char = doc.byte_to_char(end_byte);
        let start_line = doc.char_to_line(start_char);
        let end_line = doc.char_to_line(end_char.saturating_sub(1));

        for line in start_line..=end_line {
            let line_start_char = doc.line_to_char(line);
            let line_len_chars = doc.line(line).len_chars();
            let line_end_char = line_start_char + line_len_chars;

            let seg_start = start_char.max(line_start_char);
            let seg_end = end_char.min(line_end_char);

            let mut len = seg_end.saturating_sub(seg_start);
            while len > 0 {
                let last_char = doc.char(seg_start + len - 1);
                if last_char == '\n' || last_char == '\r' {
                    len -= 1;
                } else {
                    break;
                }
            }

            if len > 0 {
                let col = seg_start.saturating_sub(line_start_char);
                raw_tokens.push((line as u32, col as u32, len as u32, token_type, modifiers));
            }
        }
    }

    raw_tokens.sort_by_key(|&(line, col, ..)| (line, col));

    let mut prev_line = 0;
    let mut prev_char = 0;
    let mut data = Vec::with_capacity(raw_tokens.len());

    for (line, col, len, token_type, modifiers) in raw_tokens {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 { col.saturating_sub(prev_char) } else { col };
        prev_line = line;
        prev_char = col;

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: len,
            token_type,
            token_modifiers_bitset: modifiers,
        });
    }

    SemanticTokens { result_id: None, data }
}
