/**
 * @file Cefre grammar for tree-sitter
 * @author ben
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-nocheck

module.exports = grammar({
  name: "cfr",

  word: $ => $.ident,

  extras: ($) => [
    /\s/, // whitespace
    $.comment,
  ],

  rules: {
    source_file: $ => repeat($.statement),

    statement: $ => choice(
      $.def,
      $.expr,
      $.loop,
      $._break,
      $._continue,
      $._if,
      $.assignment,
    ),

    ident: $ => token(/[a-zA-Z]{1}[a-zA-Z0-9_]*/),

    loop: $ => seq("loop", $.label, $.block),
    _break: $ => seq("break", $.label),
    _continue: $ => seq("continue", $.label),
    _if: $ => seq(
      "if",
      $.expr,
      $.block,
      repeat(seq("else", "if", $.expr, $.block)),
      optional(seq("else", $.block)),
    ),
    assignment: $ => seq(commaSep1(field("lhs", $.ident)), "<-", $.expr),
    expr: $ => choice(
      seq(
        optional("..."),
        field("function", $.ident),
        '(',
        field("arguments", commaSep($.expr)),
        ')',
      ),
      $.ident,
      $.val
    ),
    def: $ => seq(
      "def",
      field("name", $.ident),
      '(',
      field("parameters", commaSep(field("param", $.ident))),
      ')',
      optional(seq("->", field("ret", commaSep($.ident)))),
      field("body", $.block),
    ),

    label: $ => field("label", token(/:[a-zA-Z]{1}[a-zA-Z0-9_]*/)),
    val: $ => token(choice(/0x[a-fA-F0-9]+/, /\d+/)),

    block: $ => seq('{', repeat(choice('\n', seq($.statement, '\n'))), '}'),

    comment: $ => token(seq("#", /.*/)),
  }
});

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)))
}

function commaSep(rule) {
  return optional(commaSep1(rule))
}

function nlSep1(rule) {
  return seq(rule, repeat(seq('\n', rule)))
}

function nlSep(rule) {
  return optional(nlSep1(rule))
}
