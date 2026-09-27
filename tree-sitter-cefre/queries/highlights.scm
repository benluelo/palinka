[
  "<-"
  "->"
] @operator

[
  "if"
  "else"
  "break"
  "continue"
] @keyword.control.conditional

[
  "loop"
] @keyword.control.repeat

[
  "def"
] @keyword.control.function

[
  "("
  ")"
  "{"
  "}"
] @punctuation.bracket

[
  ","
] @punctuation.delimiter

[
  "..."
] @punctuation

(def
  name: (ident) @function
)

(expr 
  function: (ident) @function
)

(expr 
  function: (ident) @function.builtin
   (#any-of? @function.builtin "add" "mul" "sub" "exp" "mod" "eq" "lt" "gt" "dread1" "dlen" "alloc" "write1" "write2" "write8" "exit" "trap")
)

param: (ident) @variable.parameter
ret: (ident) @variable.parameter

(comment) @comment  

(val) @constant.numeric.integer

; (ident) @variable
(label) @label

(assignment
  lhs: (ident) @variable
)
