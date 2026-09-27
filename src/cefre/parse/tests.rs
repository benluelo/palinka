use super::*;

#[test]
fn test_expr() {
    let raw = "
        add(
            0x01,
            mul(
                var,
                0x03
            )
        )
        ";

    let res = grammar().expr.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_loop() {
    let raw = "
    loop :a {
        add(1, 2)
    }";

    let res = grammar().loop_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_if() {
    let raw = "
      if eq(counter, 3) {
          break :a
      }
    ";

    let res = grammar().if_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_if_else() {
    let raw = "
      if eq(counter, 3) {
          break :a
      } else {
          x <- 1
      }
    ";

    let res = grammar().if_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_if_else_if() {
    let raw = "
      if eq(counter, 3) {
          break :a
      } else if 0 {
          x <- 1
      } else {
          y <- 42
      }
    ";

    let res = grammar().if_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_if_else_if_chain() {
    let raw = "
      if eq(counter, 3) {
          break :a
      } else if 0 {
          x <- 1
      } else if 0 {
          x <- 1
      } else if 0 {
          x <- 1
      } else if 0 {
          x <- 1
      } else if 0 {
          x <- 1
      } else if 0 {
          x <- 1
      } else {
          y <- 42
      }
    ";

    let res = grammar().if_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_break() {
    let raw = "
      break :a
    ";

    let res = grammar().break_.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_assignment() {
    let raw = "
    counter <- 0x00
    ";

    let res = grammar().assignment.parse(raw).unwrap();

    dbg!(res);
}

#[test]
fn test_program() {
    let raw = "
    counter <- 0x00

    loop :a {
      counter <- add(counter, 1)

      if eq(counter, 3) {
        break :a
      }
    }

    alloc(1)
    write1(0)
    exit(0, 0)
        ";

    let res = grammar().block.parse(raw).unwrap();

    dbg!(&res);

    let pretty = print_ast(&res);

    println!("{pretty}");
}

#[test]
fn statement_def() {
    let raw = "
    def f(a) {}
        ";

    let res = grammar().block.parse(raw).unwrap();

    dbg!(&res);

    let pretty = print_ast(&res);

    println!("{pretty}");
}

#[test]
fn multiline_params_def() {
    let raw = "
    def f(
      a, # a
      b, # b
      c, # c
    ) {
    }
        ";

    let res = grammar().def.parse(raw).unwrap();

    dbg!(&res);

    // let pretty = print_ast(&res);

    // println!("{pretty}");
}

#[test]
fn def_comment_after_ret() {
    let raw = "
    def f(
    ) -> r # c
    {
    }
        ";

    let res = grammar().def.parse(raw).unwrap();

    dbg!(&res);

    // let pretty = print_ast(&res);

    // println!("{pretty}");
}

#[test]
fn def_empty_body_only_comments() {
    let raw = "
    def f() {
      # c
    }
        ";

    let res = grammar().def.parse(raw).unwrap();

    dbg!(&res);

    // let pretty = print_ast(&res);

    // println!("{pretty}");
}
