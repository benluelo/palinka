#![warn(clippy::panic)]

use core::fmt;
use std::{
    cmp, fs, io,
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use argh::{FromArgValue, FromArgs};
use ariadne::{Label, Report, ReportKind, Source};
use chumsky::{Parser, error::Rich};
use const_hex::ToHexExt;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use palinka::{
    CycleCountHook, CycleCountVm, Error, Hook, Op, StepResult, Vm, VmRunResult, VmT,
    assembler::parse_asm,
    cefre::{
        self, CheckCtx, CompileResult, Ctx,
        parse::print_ast,
        pass::{ConstEval, ConstProp, DeadCodeRemoval, DefInline, LoopUnroll, MergeAlloc, Pass},
    },
    ffi,
    tail::TcVm,
    zig,
};
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Layout, Margin},
    prelude::CrosstermBackend,
    style::{Color, Style, Stylize},
    symbols::scrollbar,
    text::{Line, Span},
    widgets::{
        Block, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Compiler and assembler.
#[derive(FromArgs, PartialEq, Debug)]
struct Args {
    #[argh(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
pub enum Cmd {
    Check(CheckCmd),
    Build(BuildCmd),
    Run(RunCmd),
    Debug(DebugCmd),
}

/// check a .cfr file.
#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "check")]
pub struct CheckCmd {
    /// the file to check.
    #[argh(positional)]
    pub file: PathBuf,
}

/// build a .cfr file.
#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "build")]
pub struct BuildCmd {
    /// the file to compile.
    #[argh(positional)]
    pub file: PathBuf,

    /// if this flag is provided, the source file will be treated as a assembly
    /// file rather than a code file.
    #[argh(switch)]
    pub asm: bool,

    /// the file to write the output to.
    ///
    /// if not provided, this will default to the input file name with the file
    /// extension replaced with `.o`.
    #[argh(option, short = 'o')]
    pub out: Option<PathBuf>,

    /// what to emit. defaults to object.
    #[argh(option, default = "Emit::Object")]
    pub emit: Emit,
}

/// run either a .cfr or .asm file.
#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "run")]
pub struct RunCmd {
    /// the file to compile.
    #[argh(positional)]
    pub file: PathBuf,

    /// if this flag is provided, the source file will be treated as a assembly
    /// file rather than a code file.
    #[argh(switch)]
    pub asm: bool,

    /// if this flag is provided, the source file will be treated as an object
    /// file rather than a code file.
    #[argh(switch)]
    pub obj: bool,

    /// input to be provided to the program.
    ///
    /// Incompatible with --input-file.
    #[argh(option)]
    pub input: Option<String>,

    /// path to the input to be provided to the program when executing with
    /// --run.
    ///
    /// incompatible with --input.
    #[argh(option)]
    pub input_file: Option<PathBuf>,

    /// whether to treat --input as hex.
    #[argh(switch)]
    pub input_hex: bool,

    /// which implementation to use.
    #[argh(option, short = 'i', default = "Implementation::Rust")]
    pub implementation: Implementation,

    /// max memory.
    #[argh(option, short = 'm', default = "usize::MAX")]
    pub max_memory: usize,
}

#[derive(FromArgValue, PartialEq, Debug)]
pub enum Implementation {
    #[argh(name = "rust")]
    Rust,
    #[argh(name = "rust-tc")]
    RustTailCall,
    #[argh(name = "c")]
    CComputedGoto,
    #[argh(name = "zig")]
    Zig,
}

/// debug the execution of compiled bytecode against provided input
#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand, name = "debug")]
pub struct DebugCmd {
    /// the file to compile.
    #[argh(positional)]
    pub file: PathBuf,

    /// if this flag is provided, the source file will be treated as a assembly
    /// file rather than a code file.
    #[argh(switch)]
    pub asm: bool,

    /// if this flag is provided, the source file will be treated as an object
    /// file rather than a code file.
    #[argh(switch)]
    pub obj: bool,

    /// input to be provided to the program.
    ///
    /// incompatible with --input-file.
    #[argh(option)]
    pub input: Option<String>,

    /// path to the input to be provided to the program.
    ///
    /// incompatible with --input.
    #[argh(option)]
    pub input_file: Option<PathBuf>,

    /// whether to treat --input as hex.
    #[argh(switch)]
    pub input_hex: bool,

    /// max memory.
    #[argh(option, short = 'm', default = "usize::MAX")]
    pub max_memory: usize,
}

#[derive(Debug, Clone, PartialEq, Default, FromArgValue)]
pub enum Emit {
    Asm,
    #[default]
    Object,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    let args = argh::from_env::<Args>();

    // argh doesn't have any "arg required else help" functionality
    let Some(cmd) = args.cmd else {
        let cmd = std::env::args().next().unwrap();
        let cmd = std::path::Path::new(&cmd).file_name().and_then(|s| s.to_str()).unwrap_or(&cmd);
        let early_exit = Args::from_args(&[cmd], &["--help"]).unwrap_err();
        print!("{}", early_exit.output);
        std::process::exit(0);
    };

    match cmd {
        Cmd::Check(CheckCmd { file }) => {
            let source = fs::read_to_string(&file)?;

            match cefre::parse::grammar().block.parse(&source).into_result() {
                Ok(obj) => {
                    let mut ctx = CheckCtx::new("root");
                    ctx.check(&obj)?;
                }
                Err(errs) => {
                    report_errors(&file, &source, errs);
                }
            }
        }
        Cmd::Build(BuildCmd { file, asm, out, emit }) => {
            let source = fs::read_to_string(&file)?;

            let obj = if asm {
                match parse_asm().parse(&source).into_result() {
                    Ok(obj) => obj.assemble(),
                    Err(errs) => {
                        report_errors(&file, &source, errs);
                    }
                }
            } else {
                match cefre::parse::grammar().block.parse(&source).into_result() {
                    Ok(ast) => {
                        let ast = match optimize(ast) {
                            Ok(ast) => ast,
                            Err(err) => {
                                report_errors(&file, &source, err.into_rich());
                            }
                        };

                        // println!("{}", print_ast(&ast));

                        let mut ctx = Ctx::new_root();
                        ctx.compile(&ast)?;
                        match emit {
                            Emit::Asm => {
                                let obj = ctx.into_object();
                                match out {
                                    Some(out) => fs::write(out, obj.to_string())?,
                                    None => println!("{obj}"),
                                }
                                return Ok(());
                            }
                            Emit::Object => ctx.into_object().assemble(),
                        }
                    }
                    Err(errs) => {
                        report_errors(&file, &source, errs);
                    }
                }
            };

            let out = out.unwrap_or(file.with_extension("o"));
            fs::write(out, obj)?;
        }
        Cmd::Run(RunCmd {
            file,
            asm,
            obj,
            input,
            input_file,
            input_hex,
            implementation,
            max_memory,
        }) => {
            if obj && asm {
                bail!("--asm is incompatible with --obj")
            }

            let obj = build_obj(file, asm, obj)?;

            let data = read_input(input, input_file, input_hex)?;

            match implementation {
                Implementation::Rust => {
                    do_run(Vm::new_with(obj, data, max_memory, CycleCountHook::new()));
                }
                Implementation::RustTailCall => {
                    do_run(TcVm::new(Vm::new_with(obj, data, max_memory, CycleCountHook::new())));
                }
                Implementation::CComputedGoto => {
                    do_run(ffi::Vm::new(obj, data, max_memory));
                }
                Implementation::Zig => {
                    do_run(zig::Vm::new(obj, data, max_memory));
                }
            }
        }
        Cmd::Debug(DebugCmd { file, asm, obj, input, input_file, input_hex, max_memory }) => {
            let obj = build_obj(file, asm, obj)?;

            let data = read_input(input, input_file, input_hex)?;

            let hook = DebugHook {
                // ops_channel: sender,
                // step_wait: Arc::clone(&step_wait),
                cycles: 0,
                ops: vec![],
            };

            let vm = Vm::new_with(obj, data, max_memory, hook);

            run(App {
                should_quit: false,
                // step_wait,
                // op_channel: receiver,
                // ops: vec![],
                running: false,
                last_range: None,
                half_step: false,
                tick_rate: Duration::from_millis(200),
                mem_style: Color::Cyan.into(),
                step_result: None,
                code_scroll: 0,
                code_scroll_state: ScrollbarState::new(2),
                vm,
            })?;
        }
    }

    Ok(())
}

fn build_obj(file: PathBuf, asm: bool, obj: bool) -> Result<Vec<u8>, anyhow::Error> {
    Ok(if obj {
        fs::read(&file).with_context(|| format!("path: {}", file.display()))?
    } else if asm {
        let source = fs::read_to_string(&file)?;
        match parse_asm().parse(&source).into_result() {
            Ok(obj) => obj.assemble(),
            Err(errs) => {
                report_errors(&file, &source, errs);
            }
        }
    } else {
        let source = fs::read_to_string(&file)?;
        match cefre::parse::grammar().block.parse(&source).into_result() {
            Ok(ast) => {
                let ast = optimize(ast)?;

                // println!("{}", print_ast(&ast));
                let mut ctx = Ctx::new_root();
                ctx.compile(&ast)?;
                ctx.into_object().assemble()
            }
            Err(errs) => {
                report_errors(&file, &source, errs);
            }
        }
    })
}

fn do_run(mut vm: impl CycleCountVm) {
    let now = Instant::now();
    let res = vm.run();
    let elapsed = now.elapsed();
    println!("time: {}", elapsed.as_secs_f64());
    println!("total cycles: {}", vm.cycles());
    // println!("binary size: {}", vm.code.len());
    // println!("data size: {}", vm.data.len());
    match res {
        VmRunResult::Exit(exit) => {
            println!(
                "output: {}",
                exit.encode_hex()
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(8)
                    .map(|s| s.iter().collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        VmRunResult::Done => {
            println!("output: <no output>");
        }
        err => {
            println!("err: {err:?}");
            // println!("cycles: {}", vm.cycles);

            // println!("{}", const_hex::encode(vm.memory));
        }
    }
}

fn read_input(
    input: Option<String>,
    input_file: Option<PathBuf>,
    input_hex: bool,
) -> Result<Vec<u8>, anyhow::Error> {
    let data = match (input, input_hex, input_file) {
        (None, true, None) => bail!("--input-hex requires --input"),
        (None, false, None) => vec![],
        (None, true, Some(path)) => const_hex::decode(fs::read(path)?)?,
        (None, false, Some(path)) => {
            fs::read(&path).with_context(|| format!("path: {}", path.display()))?
        }
        (Some(input), true, None) => const_hex::decode(input)?,
        (Some(input), false, None) => input.into_bytes(),
        (Some(_), _, Some(_)) => {
            bail!("--input is mutually exclusive with --input-file")
        }
    };
    Ok(data)
}

fn optimize(ast: cefre::ast::Block<'_>) -> CompileResult<cefre::ast::Block<'_>> {
    let mut ctx = CheckCtx::new("root");
    ctx.check(&ast)?;
    let ast = DefInline::new().run(&ctx, ast);
    let mut ctx = CheckCtx::new("root");
    ctx.check(&ast)?;
    let ast = DefInline::new().run(&ctx, ast);
    let mut ast = ast;
    for _ in 1..=3 {
        let mut ctx = CheckCtx::new("root");
        let new_ast = ctx.check_with(&ast, &mut LoopUnroll)?;

        if new_ast == ast {
            info!("no loops unrolled");
            break;
        }
        ast = new_ast;

        for i in 1.. {
            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&ast, &mut ConstProp)?;

            let mut ctx = CheckCtx::new("root");
            ctx.check(&new_ast)?;
            let new_ast = DefInline::new().run(&ctx, new_ast);

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut DeadCodeRemoval)?;

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut ConstEval::new())?;

            let mut ctx = CheckCtx::new("root");
            let new_ast = ctx.check_with(&new_ast, &mut MergeAlloc)?;

            if new_ast == ast {
                info!("ran const prop/eval loop {i} times");
                break;
            } else {
                ast = new_ast;
            }
        }
    }

    #[expect(
        clippy::unwrap_in_result,
        reason = "if this errors we probably have bigger issues to deal with"
    )]
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("???").as_nanos() - 1784300000000000000;
    fs::write(format!("out-{now}.cfr"), print_ast(&ast)).unwrap();

    Ok(ast)
}

fn report_errors(file_name: &Path, file: &str, errs: Vec<Rich<'_, char>>) -> ! {
    let file_name = file_name.display().to_string();

    for e in errs {
        Report::build(ReportKind::Error, (&file_name, e.span().into_range()))
            .with_config(ariadne::Config::new().with_index_type(ariadne::IndexType::Byte))
            .with_message(e.to_string())
            .with_label(
                Label::new((&file_name, e.span().into_range()))
                    .with_message(e.reason().to_string())
                    .with_color(ariadne::Color::Red),
            )
            .finish()
            .print((&file_name, Source::from(&file)))
            .unwrap()
    }

    std::process::exit(-1);
}

pub struct DebugHook {
    // ops_channel: mpsc::Sender<Op>,
    // step_wait: Arc<(Mutex<bool>, Condvar)>,
    cycles: u64,
    ops: Vec<FullOp>,
}

impl Hook for DebugHook {
    type Error = !;

    fn pre_cycle(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn cycle(&mut self, pc: usize, op: Op, stack: &[u64], mem: &[u8]) -> Result<(), Self::Error> {
        // self.ops_channel
        //     .send(op)
        //     .expect("sending should be infallible");
        self.ops.push(full_op(pc, op, stack, mem).unwrap());
        self.cycles += 1;
        // // Wait for the thread to start up.
        // let (lock, cvar) = &*self.step_wait;
        // let mut started = lock.lock().unwrap();
        // // As long as the value inside the `Mutex<bool>` is `false`, we wait.
        // while !*started {
        //     started = cvar.wait(started).unwrap();
        // }
        Ok(())
    }

    fn post_cycle(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn full_op(pc: usize, op: Op, stack: &[u64], mem: &[u8]) -> Result<FullOp, Error> {
    macro_rules! at {
        ($at:expr) => {
            *stack.get(stack.len() - (1 + $at)).ok_or(Error::StackEmpty)?
        };
    }

    macro_rules! as_ptr {
        ($e:expr) => {{
            #[cfg(target_pointer_width = "64")]
            { $e as usize }
            #[cfg(not(target_pointer_width = "64"))]
            {
                let n = $e;
                if n > (usize::MAX as u64) {
                    return Error::PointerTooBig(n);
                } else {
                    $e as u64
                }
            }
        }};
    }

    Ok(match op {
        Op::PUSH0 => FullOp::PUSH0,
        Op::PUSH1(arr) => FullOp::PUSH1(u64_from_bytes(&arr)),
        Op::PUSH2(arr) => FullOp::PUSH2(u64_from_bytes(&arr)),
        Op::PUSH3(arr) => FullOp::PUSH3(u64_from_bytes(&arr)),
        Op::PUSH4(arr) => FullOp::PUSH4(u64_from_bytes(&arr)),
        Op::PUSH5(arr) => FullOp::PUSH5(u64_from_bytes(&arr)),
        Op::PUSH6(arr) => FullOp::PUSH6(u64_from_bytes(&arr)),
        Op::PUSH7(arr) => FullOp::PUSH7(u64_from_bytes(&arr)),
        Op::PUSH8(arr) => FullOp::PUSH8(u64_from_bytes(&arr)),
        Op::DUP => {
            let idx = as_ptr!(at!(0));
            FullOp::DUP { idx, val: at!(idx + 1) }
        }
        Op::DUP0 => FullOp::DUP0 { val: at!(0) },
        Op::SWAP => {
            let idx = as_ptr!(at!(0));
            FullOp::SWAP { idx, a_val: at!(1), b_val: at!(idx + 2) }
        }
        Op::SWAP0 => FullOp::SWAP0 { a_val: at!(0), b_val: at!(1) },
        Op::POP => FullOp::POP { val: at!(0) },
        Op::ALLOC => FullOp::ALLOC { amount: as_ptr!(at!(0)) },
        Op::WRITE1 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE1 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 1].try_into().unwrap() }
        }
        Op::WRITE2 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE2 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 2].try_into().unwrap() }
        }
        Op::WRITE3 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE3 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 3].try_into().unwrap() }
        }
        Op::WRITE4 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE4 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 4].try_into().unwrap() }
        }
        Op::WRITE5 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE5 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 5].try_into().unwrap() }
        }
        Op::WRITE6 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE6 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 6].try_into().unwrap() }
        }
        Op::WRITE7 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE7 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 7].try_into().unwrap() }
        }
        Op::WRITE8 => {
            let ptr = as_ptr!(at!(1));
            FullOp::WRITE8 { ptr, val: at!(0), prev_value: mem[ptr..ptr + 8].try_into().unwrap() }
        }
        Op::READ1 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ1 { ptr, val: mem[ptr..ptr + 1].try_into().unwrap() }
        }
        Op::READ2 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ2 { ptr, val: mem[ptr..ptr + 2].try_into().unwrap() }
        }
        Op::READ3 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ3 { ptr, val: mem[ptr..ptr + 3].try_into().unwrap() }
        }
        Op::READ4 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ4 { ptr, val: mem[ptr..ptr + 4].try_into().unwrap() }
        }
        Op::READ5 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ5 { ptr, val: mem[ptr..ptr + 5].try_into().unwrap() }
        }
        Op::READ6 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ6 { ptr, val: mem[ptr..ptr + 6].try_into().unwrap() }
        }
        Op::READ7 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ7 { ptr, val: mem[ptr..ptr + 7].try_into().unwrap() }
        }
        Op::READ8 => {
            let ptr = as_ptr!(at!(0));
            FullOp::READ8 { ptr, val: mem[ptr..ptr + 8].try_into().unwrap() }
        }
        Op::DREAD1 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD2 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD3 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD4 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD5 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD6 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD7 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DREAD8 => {
            let ptr = as_ptr!(at!(0));
            FullOp::DREAD8 { ptr, val: Default::default() }
        }
        Op::DCOPY => {
            FullOp::DCOPY { src: as_ptr!(at!(0)), dst: as_ptr!(at!(1)), len: as_ptr!(at!(2)) }
        }
        Op::DLEN => FullOp::DLEN,
        Op::ADD => FullOp::ADD { a: at!(0), b: at!(1) },
        Op::SUB => FullOp::SUB { a: at!(0), b: at!(1) },
        Op::MUL => FullOp::MUL { a: at!(0), b: at!(1) },
        Op::DIV => FullOp::DIV { a: at!(0), b: at!(1) },
        Op::EXP => FullOp::EXP { a: at!(0), b: at!(1) },
        Op::MOD => FullOp::MOD { a: at!(0), b: at!(1) },
        Op::EQ => FullOp::EQ { a: at!(0), b: at!(1) },
        Op::NEQ => FullOp::NEQ { a: at!(0), b: at!(1) },
        Op::LT => FullOp::LT { a: at!(0), b: at!(1) },
        Op::GT => FullOp::GT { a: at!(0), b: at!(1) },
        Op::NOT => FullOp::NOT { a: at!(0) },
        Op::SHL => FullOp::SHL { a: at!(0), b: at!(1) },
        Op::SHR => FullOp::SHR { a: at!(0), b: at!(1) },
        Op::NEG => FullOp::NEG { a: at!(0) },
        Op::OR => FullOp::OR { a: at!(0), b: at!(1) },
        Op::XOR => FullOp::XOR { a: at!(0), b: at!(1) },
        Op::AND => FullOp::AND { a: at!(0), b: at!(1) },
        Op::JUMP => FullOp::JUMP { old_pc: pc, dst: as_ptr!(at!(0)) },
        Op::JNZ => FullOp::JNZ { cond: at!(1), old_pc: pc, dst: as_ptr!(at!(0)) },
        Op::CALL => FullOp::CALL { old_pc: pc, new_pc: as_ptr!(at!(0)) },
        Op::EXIT => FullOp::EXIT { ptr: as_ptr!(at!(0)), len: as_ptr!(at!(1)) },
        Op::TRAP => FullOp::TRAP { code: at!(0) },
    })
}

pub struct App {
    should_quit: bool,
    // step_wait: Arc<(Mutex<bool>, Condvar)>,
    // op_channel: mpsc::Receiver<Op>,
    // ops: Vec<Op>,
    vm: Vm<DebugHook>,
    running: bool,
    last_range: Option<Range<usize>>,
    half_step: bool,
    tick_rate: Duration,
    mem_style: Style,
    step_result: Option<Result<StepResult, Error<DebugHook>>>,
    code_scroll: usize,
    code_scroll_state: ScrollbarState,
}

impl App {
    fn on_tick(&mut self) {
        // match self.op_channel.try_recv() {
        //     Ok(op) => {
        //         self.ops.push(op);
        //     }
        //     Err(TryRecvError::Empty) => {}
        //     Err(TryRecvError::Disconnected) => {
        //         self.should_quit = true;
        //     }
        // }
        if self.running {
            self.step();
        }
    }

    // fn on_left(&mut self) {}

    // fn on_up(&mut self) {}

    // fn on_right(&mut self) {}

    // fn on_down(&mut self) {}

    fn on_key(&mut self, c: char) {
        match c {
            'q' => {
                self.should_quit = true;
            }
            'g' => {
                self.running = !self.running;
                self.tick_rate = if self.running {
                    // full speed ahead
                    Duration::ZERO
                } else {
                    // but chill out if we're not running
                    Duration::from_millis(200)
                };
            }
            'n' => {
                self.step();
            }
            'b' => {
                self.half_step = !self.half_step;
            }
            'p' => {
                self.undo();
            }
            'j' => self.scroll_down(),
            'k' => self.scroll_up(),
            _ => {}
        }
    }

    fn step(&mut self) {
        self.half_step = false;
        // if no step result or last step ws successful
        if matches!(self.step_result, None | Some(Ok(StepResult::Stepped))) {
            let step = self.vm.step();
            self.step_result = Some(step.clone());
            match step {
                Ok(ok) => match ok {
                    StepResult::Stepped => {}
                    StepResult::Eof => {
                        self.running = false;
                    }
                    StepResult::Exit(_) => {
                        self.running = false;
                    }
                },
                Err(err) => {
                    // self.should_quit = true;
                    match err {
                        Error::OutOfMemory => {}
                        Error::StackEmpty => {}
                        Error::InvalidStackIdx => {}
                        Error::Segfault => {}
                        Error::Eof => {}
                        Error::DivideByZero => {}
                        Error::InvalidStackValue => {}
                        Error::Trap(_) => {}
                        Error::UnknownOp(_) => {}
                        Error::Hook(_) => {}
                    }
                }
            };
        }
    }

    fn undo(&mut self) {
        self.last_range = None;

        if let Some(op) = self.vm.hook.ops.pop() {
            macro_rules! binop {
                ($a:ident, $b:ident) => {{
                    self.vm.pc -= 1;
                    self.vm.stack.pop().unwrap();
                    self.vm.stack.push($b);
                    self.vm.stack.push($a);
                }};
            }

            self.step_result = None;
            self.vm.hook.cycles -= 1;

            match op {
                FullOp::PUSH0 => {
                    self.vm.pc -= 1;
                    self.vm.stack.pop().unwrap();
                }
                FullOp::PUSH1(val) => {
                    self.vm.pc -= 2;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH2(val) => {
                    self.vm.pc -= 3;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH3(val) => {
                    self.vm.pc -= 4;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH4(val) => {
                    self.vm.pc -= 5;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH5(val) => {
                    self.vm.pc -= 6;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH6(val) => {
                    self.vm.pc -= 7;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH7(val) => {
                    self.vm.pc -= 8;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::PUSH8(val) => {
                    self.vm.pc -= 9;
                    assert_eq!(self.vm.stack.pop().unwrap(), val);
                }
                FullOp::DUP { idx, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(val, self.vm.stack.pop().unwrap());
                    self.vm.stack.push(idx as u64);
                }
                FullOp::DUP0 { val } => {
                    self.vm.pc -= 1;
                    assert_eq!(val, self.vm.stack.pop().unwrap());
                }
                FullOp::SWAP { idx, a_val, b_val } => {
                    self.vm.pc -= 1;
                    let len = self.vm.stack.len();
                    let b_idx = len - (idx + 2);
                    let a_idx = len - 1;
                    self.vm.stack.swap(a_idx, b_idx);
                    assert_eq!(self.vm.stack[a_idx], a_val);
                    assert_eq!(self.vm.stack[b_idx], b_val);
                    self.vm.stack.push(idx as u64);
                }
                FullOp::SWAP0 { a_val, b_val } => {
                    let len = self.vm.stack.len();
                    self.vm.stack.swap(len - 1, len - 2);
                    assert_eq!(self.vm.stack[len - 1], a_val);
                    assert_eq!(self.vm.stack[len - 2], b_val);
                    self.vm.pc -= 1;
                }
                FullOp::POP { val } => {
                    self.vm.pc -= 1;
                    self.vm.stack.push(val);
                }
                FullOp::ALLOC { amount } => {
                    self.vm.pc -= 1;
                    self.vm.stack.push(amount as u64);
                    self.vm.memory.truncate(self.vm.memory.len() - amount);
                }
                FullOp::WRITE1 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 1].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE2 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 2].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE3 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 3].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE4 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 4].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE5 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 5].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE6 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 6].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE7 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 7].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::WRITE8 { ptr, val, prev_value } => {
                    self.vm.pc -= 1;
                    self.vm.memory[ptr..ptr + 8].copy_from_slice(&prev_value);
                    self.vm.stack.push(ptr as u64);
                    self.vm.stack.push(val);
                }
                FullOp::READ1 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ2 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ3 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ4 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ5 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ6 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ7 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::READ8 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD1 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD2 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD3 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD4 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD5 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD6 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD7 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DREAD8 { ptr, val } => {
                    self.vm.pc -= 1;
                    assert_eq!(self.vm.stack.pop().unwrap(), u64_from_bytes(&val));
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::DCOPY { src, dst, len } => {
                    self.vm.pc -= 1;
                    self.vm.stack.push(src as u64);
                    self.vm.stack.push(dst as u64);
                    self.vm.stack.push(len as u64);
                }
                FullOp::DLEN => {
                    self.vm.pc -= 1;
                    self.vm.stack.pop();
                }
                FullOp::ADD { a, b } => binop!(a, b),
                FullOp::SUB { a, b } => binop!(a, b),
                FullOp::MUL { a, b } => binop!(a, b),
                FullOp::DIV { a, b } => binop!(a, b),
                FullOp::EXP { a, b } => binop!(a, b),
                FullOp::MOD { a, b } => binop!(a, b),
                FullOp::EQ { a, b } => binop!(a, b),
                FullOp::NEQ { a, b } => binop!(a, b),
                FullOp::LT { a, b } => binop!(a, b),
                FullOp::GT { a, b } => binop!(a, b),
                FullOp::NOT { a } => {
                    self.vm.pc -= 1;
                    self.vm.stack.pop().unwrap();
                    self.vm.stack.push(a);
                }
                FullOp::SHL { a, b } => binop!(a, b),
                FullOp::SHR { a, b } => binop!(a, b),
                FullOp::NEG { a } => {
                    self.vm.pc -= 1;
                    self.vm.stack.pop().unwrap();
                    self.vm.stack.push(a);
                }
                FullOp::OR { a, b } => binop!(a, b),
                FullOp::XOR { a, b } => binop!(a, b),
                FullOp::AND { a, b } => binop!(a, b),
                FullOp::JUMP { old_pc, dst: new_pc } => {
                    self.vm.pc = old_pc - 1;
                    self.vm.stack.push(new_pc as u64);
                }
                FullOp::JNZ { cond, old_pc, dst: new_pc } => {
                    // if cond {
                    self.vm.pc = old_pc - 1;
                    self.vm.stack.push(cond);
                    self.vm.stack.push(new_pc as u64);
                    // } else {
                    // }
                }
                FullOp::CALL { old_pc, new_pc } => {
                    self.vm.pc = old_pc - 1;
                    self.vm.stack.pop().unwrap();
                    self.vm.stack.push(new_pc as u64);
                }
                FullOp::EXIT { ptr, len } => {
                    self.vm.pc -= 1;
                    self.vm.stack.push(len as u64);
                    self.vm.stack.push(ptr as u64);
                }
                FullOp::TRAP { code } => {
                    self.vm.pc -= 1;
                    self.vm.stack.push(code);
                }
            }
        }
    }

    const fn scroll_down(&mut self) {
        self.code_scroll = self.code_scroll.saturating_add(1);
        self.code_scroll_state = self.code_scroll_state.position(self.code_scroll);
    }

    const fn scroll_up(&mut self) {
        self.code_scroll = self.code_scroll.saturating_sub(1);
        self.code_scroll_state = self.code_scroll_state.position(self.code_scroll);
    }
}

pub fn run(mut app: App) -> anyhow::Result<()> {
    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    std::panic::set_hook(Box::new(|info| {
        let stdout = io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let Ok(mut terminal) = Terminal::new(backend) else {
            return;
        };
        if let Err(err) =
            execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)
        {
            println!("unable to reset screen: {err}");
        };
        if let Err(err) = terminal.show_cursor() {
            println!("unable to show cursor: {err}");
        };

        println!("{info}");
    }));

    // create app and run it
    let app_result = {
        let mut last_tick = Instant::now();
        loop {
            terminal.draw(|frame| render(frame, &mut app))?;

            let timeout = app.tick_rate.saturating_sub(last_tick.elapsed());
            if !event::poll(timeout)? {
                app.on_tick();
                last_tick = Instant::now();
                continue;
            }
            match event::read()? {
                event::Event::FocusGained => {}
                event::Event::FocusLost => {}
                event::Event::Key(key_event) => match key_event.code {
                    // KeyCode::Char('h') | KeyCode::Left => app.on_left(),
                    KeyCode::Char('j') | KeyCode::Down => app.scroll_up(),
                    KeyCode::Char('k') | KeyCode::Up => app.scroll_down(),
                    // KeyCode::Char('l') | KeyCode::Right => app.on_right(),
                    KeyCode::Char(c) => app.on_key(c),
                    _ => {}
                },
                event::Event::Mouse(mouse_event) => match mouse_event.kind {
                    event::MouseEventKind::Down(_mouse_button) => {}
                    event::MouseEventKind::Up(_mouse_button) => {}
                    event::MouseEventKind::Drag(_mouse_button) => {}
                    event::MouseEventKind::Moved => {}
                    event::MouseEventKind::ScrollDown => app.scroll_down(),
                    event::MouseEventKind::ScrollUp => app.scroll_up(),
                    event::MouseEventKind::ScrollLeft => {}
                    event::MouseEventKind::ScrollRight => {}
                },
                event::Event::Paste(_) => {}
                event::Event::Resize(_, _) => {}
            }
            if app.should_quit {
                break anyhow::Ok(());
            }
        }
    };

    // restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(err) = app_result {
        println!("{err:?}");
    }

    Ok(())
}

pub fn render(frame: &mut Frame, app: &mut App) {
    const READ_STYLE: Style = Style::new().fg(Color::Yellow);
    const WRITE_STYLE: Style = Style::new().fg(Color::Cyan);

    let [top, middle, bottom] = frame.area().layout(
        &Layout::vertical([
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .spacing(0),
    );
    let [left, right] = top.layout(&Layout::horizontal([Constraint::Percentage(50); 2]).spacing(0));

    frame.render_widget(
        List::new(
            app.vm
                .hook
                .ops
                .iter()
                .rev()
                // .take(10)
                .map(|op| {
                    ListItem::new(Span::from(format!("{op}")).style(op.style().not_underlined()))
                }),
        )
        .block(Block::bordered().title("ops"))
        .style(BASE_STYLE),
        left,
    );

    #[expect(unused_variables)]
    let (new_range, highlight_idxs, mem_color) = match app.vm.hook.ops.last() {
        Some(op) => match op {
            FullOp::ALLOC { amount } => {
                (Some(app.vm.memory.len() - amount..app.vm.memory.len()), vec![], app.mem_style)
            }
            FullOp::WRITE1 { ptr, val, prev_value } => (Some(*ptr..ptr + 1), vec![], WRITE_STYLE),
            FullOp::WRITE2 { ptr, val, prev_value } => (Some(*ptr..ptr + 2), vec![], WRITE_STYLE),
            FullOp::WRITE3 { ptr, val, prev_value } => (Some(*ptr..ptr + 3), vec![], WRITE_STYLE),
            FullOp::WRITE4 { ptr, val, prev_value } => (Some(*ptr..ptr + 4), vec![], WRITE_STYLE),
            FullOp::WRITE5 { ptr, val, prev_value } => (Some(*ptr..ptr + 5), vec![], WRITE_STYLE),
            FullOp::WRITE6 { ptr, val, prev_value } => (Some(*ptr..ptr + 6), vec![], WRITE_STYLE),
            FullOp::WRITE7 { ptr, val, prev_value } => (Some(*ptr..ptr + 7), vec![], WRITE_STYLE),
            FullOp::WRITE8 { ptr, val, prev_value } => (Some(*ptr..ptr + 8), vec![], WRITE_STYLE),
            FullOp::READ1 { ptr, val } => (Some(*ptr..ptr + 1), vec![], READ_STYLE),
            FullOp::READ2 { ptr, val } => (Some(*ptr..ptr + 2), vec![], READ_STYLE),
            FullOp::READ3 { ptr, val } => (Some(*ptr..ptr + 3), vec![], READ_STYLE),
            FullOp::READ4 { ptr, val } => (Some(*ptr..ptr + 4), vec![], READ_STYLE),
            FullOp::READ5 { ptr, val } => (Some(*ptr..ptr + 5), vec![], READ_STYLE),
            FullOp::READ6 { ptr, val } => (Some(*ptr..ptr + 6), vec![], READ_STYLE),
            FullOp::READ7 { ptr, val } => (Some(*ptr..ptr + 7), vec![], READ_STYLE),
            FullOp::READ8 { ptr, val } => (Some(*ptr..ptr + 8), vec![], READ_STYLE),
            FullOp::DCOPY { src, dst, len } => (Some(*dst..(dst + len)), vec![], WRITE_STYLE),
            FullOp::PUSH0 => (None, vec![0], app.mem_style),
            FullOp::PUSH1(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH2(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH3(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH4(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH5(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH6(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH7(_) => (None, vec![0], app.mem_style),
            FullOp::PUSH8(_) => (None, vec![0], app.mem_style),
            FullOp::DUP { idx, val } => (None, vec![0, (*idx) + 1], app.mem_style),
            FullOp::DUP0 { val } => (None, vec![0, 1], app.mem_style),
            FullOp::SWAP { idx, a_val, b_val } => (None, vec![0, (*idx + 1)], app.mem_style),
            FullOp::SWAP0 { a_val, b_val } => (None, vec![0, 1], app.mem_style),
            FullOp::POP { val } => (None, vec![], app.mem_style),
            FullOp::DREAD1 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD2 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD3 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD4 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD5 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD6 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD7 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DREAD8 { ptr, val } => (None, vec![0], app.mem_style),
            FullOp::DLEN => (None, vec![0], app.mem_style),
            FullOp::ADD { a, b } => (None, vec![0], app.mem_style),
            FullOp::SUB { a, b } => (None, vec![0], app.mem_style),
            FullOp::MUL { a, b } => (None, vec![0], app.mem_style),
            FullOp::DIV { a, b } => (None, vec![0], app.mem_style),
            FullOp::EXP { a, b } => (None, vec![0], app.mem_style),
            FullOp::MOD { a, b } => (None, vec![0], app.mem_style),
            FullOp::EQ { a, b } => (None, vec![0], app.mem_style),
            FullOp::NEQ { a, b } => (None, vec![0], app.mem_style),
            FullOp::LT { a, b } => (None, vec![0], app.mem_style),
            FullOp::GT { a, b } => (None, vec![0], app.mem_style),
            FullOp::NOT { a } => (None, vec![0], app.mem_style),
            FullOp::SHL { a, b } => (None, vec![0], app.mem_style),
            FullOp::SHR { a, b } => (None, vec![0], app.mem_style),
            FullOp::NEG { a } => (None, vec![0], app.mem_style),
            FullOp::OR { a, b } => (None, vec![0], app.mem_style),
            FullOp::XOR { a, b } => (None, vec![0], app.mem_style),
            FullOp::AND { a, b } => (None, vec![0], app.mem_style),
            FullOp::JUMP { old_pc, dst: new_pc } => (None, vec![], app.mem_style),
            FullOp::JNZ { cond, old_pc, dst: new_pc } => (None, vec![], app.mem_style),
            FullOp::CALL { old_pc, new_pc } => (None, vec![], app.mem_style),
            FullOp::EXIT { ptr, len } => (None, vec![], app.mem_style),
            FullOp::TRAP { code } => (None, vec![], app.mem_style),
        },
        None => (None, vec![], app.mem_style),
    };

    app.mem_style = mem_color;

    let range = new_range.clone().or(app.last_range.clone()).unwrap_or(0..0);

    if let Some(new_range) = new_range.clone() {
        app.last_range = Some(new_range);
    }

    frame.render_widget(
        Paragraph::new(Line::from_iter([
            Span::from((&app.vm.memory[..range.start]).encode_hex()),
            Span::from((&app.vm.memory.get(range.clone()).unwrap_or_default()).encode_hex())
                .style(if new_range.is_some() { mem_color.reversed() } else { mem_color }),
            Span::from((&app.vm.memory.get(range.end..).unwrap_or_default()).encode_hex()),
        ]))
        .wrap(Wrap { trim: false })
        .block(Block::bordered().title(format!("memory (len: {})", app.vm.memory.len())))
        .style(BASE_STYLE),
        middle,
    );

    // NOTE: Ensure these never overlap
    let mut op_ranges = if let Some(Ok(StepResult::Eof)) = app.step_result {
        vec![(app.vm.pc..app.vm.code.len(), Style::new().cyan())]
    } else {
        // 0011223344
        // ^ ^   ^ ^ pc
        // | |---| data
        // | op
        let pushn = |n| {
            vec![
                ((app.vm.pc - (n + 1))..(app.vm.pc - n), PUSH_STYLE),
                ((app.vm.pc - n)..(app.vm.pc), PUSH_STYLE_BZ),
            ]
        };

        let basic_op = |style| vec![((app.vm.pc - 1)..app.vm.pc, style)];

        match app.vm.hook.ops.last() {
            Some(op) => match op {
                FullOp::PUSH1(_) => pushn(1),
                FullOp::PUSH2(_) => pushn(2),
                FullOp::PUSH3(_) => pushn(3),
                FullOp::PUSH4(_) => pushn(4),
                FullOp::PUSH5(_) => pushn(5),
                FullOp::PUSH6(_) => pushn(6),
                FullOp::PUSH7(_) => pushn(7),
                FullOp::PUSH8(_) => pushn(8),
                FullOp::JUMP { old_pc, dst } => {
                    vec![((old_pc - 1)..*old_pc, op.style()), (*dst..dst + 1, DST_OK_STYLE)]
                }
                FullOp::JNZ { old_pc, dst, cond } => {
                    vec![
                        ((old_pc - 1)..*old_pc, op.style()),
                        (
                            *dst..dst + 1,
                            if *cond > 0 { DST_OK_STYLE } else { DST_NOK_STYLE }.underlined(),
                        ),
                        (
                            *old_pc..(old_pc + 1),
                            if *cond > 0 { DST_NOK_STYLE } else { DST_OK_STYLE },
                        ),
                    ]
                }
                FullOp::CALL { old_pc, new_pc } => {
                    vec![((old_pc - 1)..*old_pc, op.style()), (*new_pc..new_pc + 1, DST_OK_STYLE)]
                }
                op => basic_op(op.style()),
            },
            None => vec![(0..0, Style::new().dim())],
        }
    };

    op_ranges.sort_by_key(|r| r.0.start);
    let mut op_ranges = if op_ranges.len() == 1 {
        op_ranges
    } else {
        op_ranges
            .array_windows()
            .cloned()
            .flat_map(|[a, b]| [a.clone(), (a.0.end..b.0.start, Style::new().dim()), b])
            .collect::<Vec<_>>()
    };
    op_ranges.insert(0, (0..op_ranges.first().unwrap().0.start, Style::new().dim()));
    op_ranges.push((op_ranges.last().unwrap().0.end..app.vm.code.len(), Style::new().dim()));

    frame.render_widget(
        Paragraph::new(Line::from_iter(
            // [Span::from(format!(
            //     "{:?}, {:?} {op_ranges:?} ",
            //     app.step_result,
            //     app.vm.hook.ops.last()
            // ))]
            [].into_iter().chain(op_ranges.iter().filter(|r| !r.0.is_empty()).map(|a| {
                Span::from((&app.vm.code.get(a.0.start..a.0.end).unwrap_or_default()).encode_hex())
                    .style(a.1)
                // Span::from((&app.vm.code.get(a.0.end..b.0.start).
                // unwrap_or_default()). encode_hex())     .reset(),
            })),
        ))
        .wrap(Wrap { trim: false })
        .block(
            Block::bordered()
                .title(format!("code (pc: {}, cycles: {}) ", app.vm.pc, app.vm.hook.cycles)),
        )
        .style(BASE_STYLE)
        .scroll((app.code_scroll as u16, 0)),
        bottom,
    );

    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .begin_symbol(None)
            .track_symbol(None)
            .end_symbol(None),
        bottom.inner(Margin { vertical: 1, horizontal: 0 }),
        &mut app.code_scroll_state,
    );

    let content_length = (app.vm.code.len() * 2) / bottom.width as usize;
    app.code_scroll_state = app.code_scroll_state.content_length(content_length);
    app.code_scroll = cmp::min(app.code_scroll, content_length);

    frame.render_widget(
        List::new(
            app.vm
                .stack
                .iter()
                .rev()
                .enumerate()
                // .take(10)
                .map(|(idx, n)| {
                    ListItem::new(Line::from_iter([
                        Span::from(format!("0x{n:0>16x} ")).style(
                            if highlight_idxs.contains(&idx) {
                                Style::new().cyan()
                            } else {
                                Style::new()
                            },
                        ),
                        Span::from(format!("{n}")).dim(),
                    ]))
                }),
        )
        .block(Block::bordered().title("stack"))
        .style(BASE_STYLE),
        right,
    );
}

#[derive(Debug)]
#[allow(clippy::upper_case_acronyms, dead_code)]
enum FullOp {
    PUSH0,
    PUSH1(u64),
    PUSH2(u64),
    PUSH3(u64),
    PUSH4(u64),
    PUSH5(u64),
    PUSH6(u64),
    PUSH7(u64),
    PUSH8(u64),
    DUP { idx: usize, val: u64 },
    DUP0 { val: u64 },
    SWAP { idx: usize, a_val: u64, b_val: u64 },
    SWAP0 { a_val: u64, b_val: u64 },
    POP { val: u64 },
    ALLOC { amount: usize },
    WRITE1 { ptr: usize, val: u64, prev_value: [u8; 1] },
    WRITE2 { ptr: usize, val: u64, prev_value: [u8; 2] },
    WRITE3 { ptr: usize, val: u64, prev_value: [u8; 3] },
    WRITE4 { ptr: usize, val: u64, prev_value: [u8; 4] },
    WRITE5 { ptr: usize, val: u64, prev_value: [u8; 5] },
    WRITE6 { ptr: usize, val: u64, prev_value: [u8; 6] },
    WRITE7 { ptr: usize, val: u64, prev_value: [u8; 7] },
    WRITE8 { ptr: usize, val: u64, prev_value: [u8; 8] },
    READ1 { ptr: usize, val: [u8; 1] },
    READ2 { ptr: usize, val: [u8; 2] },
    READ3 { ptr: usize, val: [u8; 3] },
    READ4 { ptr: usize, val: [u8; 4] },
    READ5 { ptr: usize, val: [u8; 5] },
    READ6 { ptr: usize, val: [u8; 6] },
    READ7 { ptr: usize, val: [u8; 7] },
    READ8 { ptr: usize, val: [u8; 8] },
    DREAD1 { ptr: usize, val: [u8; 1] },
    DREAD2 { ptr: usize, val: [u8; 2] },
    DREAD3 { ptr: usize, val: [u8; 3] },
    DREAD4 { ptr: usize, val: [u8; 4] },
    DREAD5 { ptr: usize, val: [u8; 5] },
    DREAD6 { ptr: usize, val: [u8; 6] },
    DREAD7 { ptr: usize, val: [u8; 7] },
    DREAD8 { ptr: usize, val: [u8; 8] },
    DCOPY { src: usize, dst: usize, len: usize },
    DLEN,
    ADD { a: u64, b: u64 },
    SUB { a: u64, b: u64 },
    MUL { a: u64, b: u64 },
    DIV { a: u64, b: u64 },
    EXP { a: u64, b: u64 },
    MOD { a: u64, b: u64 },
    EQ { a: u64, b: u64 },
    NEQ { a: u64, b: u64 },
    LT { a: u64, b: u64 },
    GT { a: u64, b: u64 },
    NOT { a: u64 },
    SHL { a: u64, b: u64 },
    SHR { a: u64, b: u64 },
    NEG { a: u64 },
    OR { a: u64, b: u64 },
    XOR { a: u64, b: u64 },
    AND { a: u64, b: u64 },
    JUMP { old_pc: usize, dst: usize },
    JNZ { cond: u64, old_pc: usize, dst: usize },
    CALL { old_pc: usize, new_pc: usize },
    EXIT { ptr: usize, len: usize },
    TRAP { code: u64 },
}

impl FullOp {
    fn style(&self) -> Style {
        match self {
            FullOp::PUSH0 => PUSH_STYLE,
            FullOp::PUSH1(_) => PUSH_STYLE,
            FullOp::PUSH2(_) => PUSH_STYLE,
            FullOp::PUSH3(_) => PUSH_STYLE,
            FullOp::PUSH4(_) => PUSH_STYLE,
            FullOp::PUSH5(_) => PUSH_STYLE,
            FullOp::PUSH6(_) => PUSH_STYLE,
            FullOp::PUSH7(_) => PUSH_STYLE,
            FullOp::PUSH8(_) => PUSH_STYLE,
            FullOp::DUP { .. } => STACK_STYLE,
            FullOp::DUP0 { .. } => STACK_STYLE,
            FullOp::SWAP { .. } => STACK_STYLE,
            FullOp::SWAP0 { .. } => STACK_STYLE,
            FullOp::POP { .. } => STACK_STYLE,
            FullOp::ALLOC { .. } => MEM_STYLE,
            FullOp::WRITE1 { .. } => MEM_STYLE,
            FullOp::WRITE2 { .. } => MEM_STYLE,
            FullOp::WRITE3 { .. } => MEM_STYLE,
            FullOp::WRITE4 { .. } => MEM_STYLE,
            FullOp::WRITE5 { .. } => MEM_STYLE,
            FullOp::WRITE6 { .. } => MEM_STYLE,
            FullOp::WRITE7 { .. } => MEM_STYLE,
            FullOp::WRITE8 { .. } => MEM_STYLE,
            FullOp::READ1 { .. } => MEM_STYLE,
            FullOp::READ2 { .. } => MEM_STYLE,
            FullOp::READ3 { .. } => MEM_STYLE,
            FullOp::READ4 { .. } => MEM_STYLE,
            FullOp::READ5 { .. } => MEM_STYLE,
            FullOp::READ6 { .. } => MEM_STYLE,
            FullOp::READ7 { .. } => MEM_STYLE,
            FullOp::READ8 { .. } => MEM_STYLE,
            FullOp::DREAD1 { .. } => MEM_STYLE,
            FullOp::DREAD2 { .. } => MEM_STYLE,
            FullOp::DREAD3 { .. } => MEM_STYLE,
            FullOp::DREAD4 { .. } => MEM_STYLE,
            FullOp::DREAD5 { .. } => MEM_STYLE,
            FullOp::DREAD6 { .. } => MEM_STYLE,
            FullOp::DREAD7 { .. } => MEM_STYLE,
            FullOp::DREAD8 { .. } => MEM_STYLE,
            FullOp::DCOPY { .. } => MEM_STYLE,
            FullOp::DLEN => Style::new().white(),
            FullOp::ADD { .. } => ARITH_STYLE,
            FullOp::SUB { .. } => ARITH_STYLE,
            FullOp::MUL { .. } => ARITH_STYLE,
            FullOp::DIV { .. } => ARITH_STYLE,
            FullOp::EXP { .. } => ARITH_STYLE,
            FullOp::MOD { .. } => ARITH_STYLE,
            FullOp::EQ { .. } => ARITH_STYLE,
            FullOp::NEQ { .. } => ARITH_STYLE,
            FullOp::LT { .. } => ARITH_STYLE,
            FullOp::GT { .. } => ARITH_STYLE,
            FullOp::NOT { .. } => ARITH_STYLE,
            FullOp::SHL { .. } => ARITH_STYLE,
            FullOp::SHR { .. } => ARITH_STYLE,
            FullOp::NEG { .. } => ARITH_STYLE,
            FullOp::OR { .. } => ARITH_STYLE,
            FullOp::XOR { .. } => ARITH_STYLE,
            FullOp::AND { .. } => ARITH_STYLE,
            FullOp::JUMP { .. } => JUMP_STYLE,
            FullOp::JNZ { .. } => JUMP_STYLE,
            FullOp::CALL { .. } => JUMP_STYLE,
            FullOp::EXIT { .. } => EXIT_STYLE,
            FullOp::TRAP { .. } => EXIT_STYLE,
        }
    }
}

const FOREGROUND: Color = Color::from_u32(0xF6F4F4);
const BACKGROUND: Color = Color::from_u32(0x181210);

const BASE_STYLE: Style = Style::new().fg(FOREGROUND).bg(BACKGROUND);

// blue
const PUSH_STYLE: Style = Style::new().fg(Color::from_u32(0x0FBBFF));
const PUSH_STYLE_BZ: Style = Style::new().fg(Color::from_u32(0x009DDB));
// green
const STACK_STYLE: Style = Style::new().fg(Color::from_u32(0x5AC700));
const ARITH_STYLE: Style = Style::new().fg(Color::from_u32(0xFFD70F));
const MEM_STYLE: Style = Style::new().fg(Color::from_u32(0xFF9100));
const EXIT_STYLE: Style = Style::new().fg(Color::from_u32(0xF53D3D));
const JUMP_STYLE: Style = Style::new().fg(Color::from_u32(0xF514EE));
const DST_OK_STYLE: Style = Style::new().underlined().not_dim();
const DST_NOK_STYLE: Style = Style::new().crossed_out().not_dim();

#[allow(unused_variables)]
impl fmt::Display for FullOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PUSH0 => f.write_str("PUSH0"),
            Self::PUSH1(n) => f.write_fmt(format_args!("PUSH1 0x{n:x}")),
            Self::PUSH2(n) => f.write_fmt(format_args!("PUSH2 0x{n:x}")),
            Self::PUSH3(n) => f.write_fmt(format_args!("PUSH3 0x{n:x}")),
            Self::PUSH4(n) => f.write_fmt(format_args!("PUSH4 0x{n:x}")),
            Self::PUSH5(n) => f.write_fmt(format_args!("PUSH5 0x{n:x}")),
            Self::PUSH6(n) => f.write_fmt(format_args!("PUSH6 0x{n:x}")),
            Self::PUSH7(n) => f.write_fmt(format_args!("PUSH7 0x{n:x}")),
            Self::PUSH8(n) => f.write_fmt(format_args!("PUSH8 0x{n:x}")),
            Self::DUP { idx, val } => write!(f, "DUP {idx} => {val}"),
            Self::DUP0 { val } => write!(f, "DUP0 => {val}"),
            Self::SWAP { idx, a_val, b_val } => write!(f, "SWAP {idx} => {a_val} {b_val}"),
            Self::SWAP0 { a_val, b_val } => write!(f, "SWAP0 => {a_val} {b_val}"),
            Self::POP { val } => write!(f, "POP {val}"),
            Self::ALLOC { amount } => write!(f, "ALLOC {amount}"),
            Self::WRITE1 { ptr, val, prev_value } => write!(f, "WRITE1 @ {ptr} {val}"),
            Self::WRITE2 { ptr, val, prev_value } => write!(f, "WRITE2 @ {ptr} {val}"),
            Self::WRITE3 { ptr, val, prev_value } => write!(f, "WRITE3 @ {ptr} {val}"),
            Self::WRITE4 { ptr, val, prev_value } => write!(f, "WRITE4 @ {ptr} {val}"),
            Self::WRITE5 { ptr, val, prev_value } => write!(f, "WRITE5 @ {ptr} {val}"),
            Self::WRITE6 { ptr, val, prev_value } => write!(f, "WRITE6 @ {ptr} {val}"),
            Self::WRITE7 { ptr, val, prev_value } => write!(f, "WRITE7 @ {ptr} {val}"),
            Self::WRITE8 { ptr, val, prev_value } => write!(f, "WRITE8 @ {ptr} {val}"),
            Self::READ1 { ptr, val } => write!(f, "READ1 @ {ptr}"),
            Self::READ2 { ptr, val } => write!(f, "READ2 @ {ptr}"),
            Self::READ3 { ptr, val } => write!(f, "READ3 @ {ptr}"),
            Self::READ4 { ptr, val } => write!(f, "READ4 @ {ptr}"),
            Self::READ5 { ptr, val } => write!(f, "READ5 @ {ptr}"),
            Self::READ6 { ptr, val } => write!(f, "READ6 @ {ptr}"),
            Self::READ7 { ptr, val } => write!(f, "READ7 @ {ptr}"),
            Self::READ8 { ptr, val } => write!(f, "READ8 @ {ptr}"),
            Self::DREAD1 { ptr, val } => write!(f, "DREAD1 @ {ptr}"),
            Self::DREAD2 { ptr, val } => write!(f, "DREAD2 @ {ptr}"),
            Self::DREAD3 { ptr, val } => write!(f, "DREAD3 @ {ptr}"),
            Self::DREAD4 { ptr, val } => write!(f, "DREAD4 @ {ptr}"),
            Self::DREAD5 { ptr, val } => write!(f, "DREAD5 @ {ptr}"),
            Self::DREAD6 { ptr, val } => write!(f, "DREAD6 @ {ptr}"),
            Self::DREAD7 { ptr, val } => write!(f, "DREAD7 @ {ptr}"),
            Self::DREAD8 { ptr, val } => write!(f, "DREAD8 @ {ptr}"),
            Self::DCOPY { src, dst, len } => write!(f, "DCOPY"),
            Self::DLEN => write!(f, "DLEN"),
            Self::ADD { a, b } => write!(f, "ADD {a} {b}"),
            Self::SUB { a, b } => write!(f, "SUB {a} {b}"),
            Self::MUL { a, b } => write!(f, "MUL {a} {b}"),
            Self::DIV { a, b } => write!(f, "DIV {a} {b}"),
            Self::EXP { a, b } => write!(f, "EXP {a} {b}"),
            Self::MOD { a, b } => write!(f, "MOD {a} {b}"),
            Self::EQ { a, b } => write!(f, "EQ {a} {b}"),
            Self::NEQ { a, b } => write!(f, "NEQ {a} {b}"),
            Self::LT { a, b } => write!(f, "LT {a} {b}"),
            Self::GT { a, b } => write!(f, "GT {a} {b}"),
            Self::NOT { a } => write!(f, "NOT {a}"),
            Self::SHL { a, b } => write!(f, "SHL {a} {b}"),
            Self::SHR { a, b } => write!(f, "SHR {a} {b}"),
            Self::NEG { a } => write!(f, "NEG {a}"),
            Self::OR { a, b } => write!(f, "OR {a} {b}"),
            Self::XOR { a, b } => write!(f, "XOR {a} {b}"),
            Self::AND { a, b } => write!(f, "AND {a} {b}"),
            Self::JUMP { old_pc, dst: new_pc } => write!(f, "JUMP {new_pc}"),
            Self::JNZ { cond, old_pc, dst } => {
                write!(f, "JNZ if {cond} to {dst} (old_pc: {old_pc})")
            }
            Self::CALL { old_pc, new_pc } => write!(f, "CALL {new_pc}"),
            Self::EXIT { ptr, len } => write!(f, "EXIT {ptr} {len}"),
            Self::TRAP { code } => write!(f, "TRAP {code}"),
        }
    }
}

#[inline(always)]
fn u64_from_bytes(arr: &[u8]) -> u64 {
    let mut v = [0; 8];
    v[8 - arr.len()..].copy_from_slice(arr);
    u64::from_be_bytes(v)
}
