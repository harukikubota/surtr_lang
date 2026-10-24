#include "tree_sitter/parser.h"

enum TokenType { DOUBLE_CONTENT, SINGLE_CONTENT, RAW_CONTENT, ANNOTATION };

void *tree_sitter_surtr_external_scanner_create(void) { return NULL; }
void tree_sitter_surtr_external_scanner_destroy(void *payload) { (void)payload; }
unsigned tree_sitter_surtr_external_scanner_serialize(void *payload, char *buffer) {
    (void)payload; (void)buffer; return 0;
}
void tree_sitter_surtr_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
    (void)payload; (void)buffer; (void)length;
}

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static bool ascii_alpha(int32_t c) {
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
}

bool tree_sitter_surtr_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid) {
    (void)payload;
    // During error recovery all external symbols can be valid. Do not invent
    // a string state: the enclosing grammar rule owns its delimiter.
    if (valid[DOUBLE_CONTENT] && valid[SINGLE_CONTENT]) return false;
    bool raw = valid[RAW_CONTENT];
    if (raw || valid[DOUBLE_CONTENT] || valid[SINGLE_CONTENT]) {
        int32_t quote = valid[SINGLE_CONTENT] ? '\'' : '"';
        bool consumed = false;
        lexer->result_symbol = raw ? RAW_CONTENT : valid[SINGLE_CONTENT] ? SINGLE_CONTENT : DOUBLE_CONTENT;
        while (!lexer->eof(lexer)) {
            lexer->mark_end(lexer);
            if (lexer->lookahead == quote) {
                if (!raw) return consumed;
                advance(lexer);
                if (lexer->lookahead == '"') {
                    advance(lexer);
                    if (lexer->lookahead == '"') return consumed;
                }
                consumed = true;
                continue;
            }
            if (lexer->lookahead == '\\') {
                if (!raw) return consumed;
                advance(lexer);
                // In raw strings only \#{ suppresses interpolation.
                if (lexer->lookahead == '#') {
                    advance(lexer);
                    if (lexer->lookahead == '{') advance(lexer);
                }
                consumed = true;
                continue;
            }
            if (lexer->lookahead == '#') {
                advance(lexer);
                if (lexer->lookahead == '{') return consumed;
                consumed = true;
                continue;
            }
            advance(lexer);
            consumed = true;
        }
        lexer->mark_end(lexer);
        return consumed;
    }
    if (valid[ANNOTATION] && lexer->get_column(lexer) == 0) {
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t') lexer->advance(lexer, true);
        if (lexer->lookahead != '@') return false;
        advance(lexer);
        if (!ascii_alpha(lexer->lookahead)) return false;
        do { advance(lexer); } while (ascii_alpha(lexer->lookahead) ||
            (lexer->lookahead >= '0' && lexer->lookahead <= '9') || lexer->lookahead == '_');
        lexer->mark_end(lexer);
        lexer->result_symbol = ANNOTATION;
        return true;
    }
    return false;
}
