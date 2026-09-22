// Minimal `Node.Process` FFI: only process termination is ported so far.
use std::io::Write;

fn terminate(code: i32) -> ! {
    // Node flushes stdio before exiting; keep the same behaviour for the
    // reporting tests, which write their assertions to stdout.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code);
}

pub fn Node_Process_exit() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Static(|_| terminate(0)))
}

pub fn Node_Process_exitImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(std::rc::Rc::new(|code| {
        terminate(code.unwrap_int() as i32)
    })))
}
