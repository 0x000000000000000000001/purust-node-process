// Native `process` object: environment, signals, resource usage, the standard
// streams and process lifetime. Values come from `std`/libc so observable
// behaviour matches the JavaScript FFI this package replaces on Unix.
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

use Purs_Node_EventEmitter::{purust_emitter_box, purust_emitter_emit, EventEmitter};

/// Foreign type `Node.Process.Process`. The process object is an event emitter,
/// like in Node; native state lives in the emitter's user data / statics.
pub type Process = EventEmitter;

/// `getExitCode` returns `null` until an exit code has been set, so an
/// impossible exit status doubles as the unset marker.
const EXIT_CODE_UNSET: i64 = i64::MIN;

static EXIT_CODE: AtomicI64 = AtomicI64::new(EXIT_CODE_UNSET);
static HAS_UNCAUGHT_CALLBACK: AtomicBool = AtomicBool::new(false);
static UNCAUGHT_CALLBACK: Mutex<Option<crate::UnknownType>> = Mutex::new(None);
static TITLE: Mutex<Option<String>> = Mutex::new(None);
static PROCESS: OnceLock<Rc<EventEmitter>> = OnceLock::new();
static STDIN: OnceLock<Rc<EventEmitter>> = OnceLock::new();
static STDOUT: OnceLock<Rc<EventEmitter>> = OnceLock::new();
static STDERR: OnceLock<Rc<EventEmitter>> = OnceLock::new();
static START: OnceLock<std::time::Instant> = OnceLock::new();
static ENV: OnceLock<Rc<Purs_Foreign_Object::Object>> = OnceLock::new();

fn string(text: &str) -> crate::UnknownType {
    crate::Value::String(purust_core::purust_string_from_utf8(text))
}

fn effect(build: impl Fn() -> crate::UnknownType + 'static) -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(move |_| build())))
}

fn effect_string(text: String) -> crate::UnknownType {
    effect(move || string(&text))
}

fn effect_int(value: i64) -> crate::UnknownType {
    effect(move || crate::mk_int(value))
}

fn effect_bool(value: bool) -> crate::UnknownType {
    effect(move || crate::mk_bool(value))
}

fn nullable(value: Option<crate::UnknownType>) -> Rc<Purs_Data_Nullable::Nullable> {
    match value {
        Some(value) => Purs_Data_Nullable::Data_Nullable_notNull(value),
        None => Purs_Data_Nullable::Data_Nullable_null(),
    }
}

fn record(fields: Vec<(&str, crate::UnknownType)>) -> crate::UnknownType {
    let mut record = purust_core::RecordFields::new();
    for (key, value) in fields {
        record.insert(key.to_owned(), value);
    }
    crate::Value::DynamicRecord(perceus_ptr::PerceusPtr::new(record))
}

/// `Foreign.Object` handles are class-boxed so `__purust_foreign_object` finds
/// the native `Rc<SharedRecord>` payload.
fn object_value(object: Rc<Purs_Foreign_Object::Object>) -> crate::UnknownType {
    crate::Value::Class(Rc::new(object))
}

fn env_object() -> Rc<Purs_Foreign_Object::Object> {
    ENV.get_or_init(|| {
        Rc::new(Purs_Foreign_Object::Object::from_entries(
            std::env::vars()
                .map(|(key, value)| {
                    (
                        key,
                        crate::Value::String(purust_core::purust_string_from_utf8(&value)),
                    )
                })
                .collect(),
        ))
    })
    .clone()
}

fn process_emitter() -> Rc<EventEmitter> {
    PROCESS
        .get_or_init(|| Rc::new(EventEmitter::new_native()))
        .clone()
}

fn exit_code() -> i64 {
    let code = EXIT_CODE.load(Ordering::SeqCst);
    if code == EXIT_CODE_UNSET { 0 } else { code }
}

fn terminate(code: i64) -> ! {
    // Node flushes stdio before exiting; keep the same behaviour for the
    // reporting tests, which write their assertions to stdout.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    std::process::exit(code as i32);
}

/// Resident set size in bytes. `getrusage` reports the maximum RSS, which is
/// the closest native equivalent of Node's rss on both supported profiles.
fn resident_set_size() -> i64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(pages) = statm.split_whitespace().nth(1).and_then(|v| v.parse::<i64>().ok()) {
                return pages * 4096;
            }
        }
    }
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
            #[cfg(target_os = "macos")]
            {
                return usage.ru_maxrss.max(0);
            }
            #[cfg(not(target_os = "macos"))]
            {
                return usage.ru_maxrss.max(0) * 1024;
            }
        }
    }
    0
}

fn signal_number(name: &str) -> Option<i32> {
    #[cfg(unix)]
    {
        let upper = name.to_ascii_uppercase();
        match upper.as_str() {
            "SIGHUP" => Some(libc::SIGHUP),
            "SIGINT" => Some(libc::SIGINT),
            "SIGQUIT" => Some(libc::SIGQUIT),
            "SIGILL" => Some(libc::SIGILL),
            "SIGTRAP" => Some(libc::SIGTRAP),
            "SIGABRT" | "SIGIOT" => Some(libc::SIGABRT),
            "SIGBUS" => Some(libc::SIGBUS),
            "SIGFPE" => Some(libc::SIGFPE),
            "SIGKILL" => Some(libc::SIGKILL),
            "SIGUSR1" => Some(libc::SIGUSR1),
            "SIGSEGV" => Some(libc::SIGSEGV),
            "SIGUSR2" => Some(libc::SIGUSR2),
            "SIGPIPE" => Some(libc::SIGPIPE),
            "SIGALRM" => Some(libc::SIGALRM),
            "SIGTERM" => Some(libc::SIGTERM),
            "SIGCHLD" => Some(libc::SIGCHLD),
            "SIGCONT" => Some(libc::SIGCONT),
            "SIGSTOP" => Some(libc::SIGSTOP),
            "SIGTSTP" => Some(libc::SIGTSTP),
            "SIGTTIN" => Some(libc::SIGTTIN),
            "SIGTTOU" => Some(libc::SIGTTOU),
            "SIGURG" => Some(libc::SIGURG),
            "SIGXCPU" => Some(libc::SIGXCPU),
            "SIGXFSZ" => Some(libc::SIGXFSZ),
            "SIGVTALRM" => Some(libc::SIGVTALRM),
            "SIGPROF" => Some(libc::SIGPROF),
            "SIGWINCH" => Some(libc::SIGWINCH),
            "SIGIO" => Some(libc::SIGIO),
            "SIGSYS" => Some(libc::SIGSYS),
            _ => None,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        None
    }
}

fn kill(pid: i64, signal: i32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, signal) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
        false
    }
}

fn fd_is_tty(fd: i32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::isatty(fd) == 1 }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        false
    }
}

fn stdin_stream() -> Rc<EventEmitter> {
    STDIN
        .get_or_init(|| {
            let stream = Purs_Node_Stream::purust_stream_new_stream(true, false);
            // Read fd 0 off the main thread and deliver chunks through the
            // captured microtask queue so PS callbacks never run on it.
            let queue = purust_core::microtasks::current();
            let reader = stream.clone();
            std::thread::spawn(move || {
                use std::io::Read;
                let mut stdin = std::io::stdin();
                let mut buffer = [0u8; 65536];
                loop {
                    match stdin.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            let bytes = buffer[..count].to_vec();
                            let reader = reader.clone();
                            queue.enqueue(move || {
                                Purs_Node_Stream::purust_stream_push(&reader, bytes);
                            });
                        }
                    }
                }
                queue.enqueue(move || {
                    Purs_Node_Stream::purust_stream_end(&reader);
                });
            });
            stream
        })
        .clone()
}

fn stdio_stream(fd: i32, slot: &'static OnceLock<Rc<EventEmitter>>) -> Rc<EventEmitter> {
    slot.get_or_init(|| {
        let stream = Purs_Node_Stream::purust_stream_new_stream(false, true);
        Purs_Node_Stream::purust_stream_set_write_fd(&stream, fd);
        stream
    })
    .clone()
}

// ---------------------------------------------------------------------------
// Process object and lifetime
// ---------------------------------------------------------------------------

pub fn Node_Process_process() -> Rc<Process> {
    process_emitter()
}

pub fn Node_Process_exit() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Static(|_| {
        let code = exit_code();
        purust_emitter_emit(&process_emitter(), "exit", vec![crate::mk_int(code)]);
        terminate(code)
    }))
}

pub fn Node_Process_exitImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|code| {
        let code = code.unwrap_int();
        purust_emitter_emit(&process_emitter(), "exit", vec![crate::mk_int(code)]);
        terminate(code)
    })))
}

pub fn Node_Process_abortImpl() -> Rc<Purs_Data_Nullable::Nullable> {
    nullable(Some(crate::Value::Func1(purust_core::Func1::Static(|_| {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::abort()
    }))))
}

pub fn Node_Process_setExitCodeImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|code| {
        EXIT_CODE.store(code.unwrap_int(), Ordering::SeqCst);
        crate::Value::Unit
    })))
}

pub fn Node_Process_getExitCodeImpl() -> crate::UnknownType {
    effect(move || {
        let code = EXIT_CODE.load(Ordering::SeqCst);
        crate::Value::Class(Rc::new(if code == EXIT_CODE_UNSET {
            Purs_Data_Nullable::Data_Nullable_null()
        } else {
            Purs_Data_Nullable::Data_Nullable_notNull(crate::mk_int(code))
        }))
    })
}

pub fn Node_Process_getGidImpl() -> crate::UnknownType {
    effect(move || {
        #[cfg(unix)]
        {
            crate::Value::Class(Rc::new(Purs_Data_Nullable::Data_Nullable_notNull(
                crate::mk_int(unsafe { libc::getgid() } as i64),
            )))
        }
        #[cfg(not(unix))]
        {
            crate::Value::Class(Rc::new(Purs_Data_Nullable::Data_Nullable_null()))
        }
    })
}

pub fn Node_Process_getUidImpl() -> crate::UnknownType {
    effect(move || {
        #[cfg(unix)]
        {
            crate::Value::Class(Rc::new(Purs_Data_Nullable::Data_Nullable_notNull(
                crate::mk_int(unsafe { libc::getuid() } as i64),
            )))
        }
        #[cfg(not(unix))]
        {
            crate::Value::Class(Rc::new(Purs_Data_Nullable::Data_Nullable_null()))
        }
    })
}

pub fn Node_Process_hasUncaughtExceptionCaptureCallback() -> crate::UnknownType {
    effect(move || crate::mk_bool(HAS_UNCAUGHT_CALLBACK.load(Ordering::SeqCst)))
}

pub fn Node_Process_setUncaughtExceptionCaptureCallbackImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|callback| {
        *UNCAUGHT_CALLBACK.lock().unwrap() = Some(callback);
        HAS_UNCAUGHT_CALLBACK.store(true, Ordering::SeqCst);
        crate::Value::Unit
    })))
}

pub fn Node_Process_clearUncaughtExceptionCaptureCallback() -> crate::UnknownType {
    effect(move || {
        *UNCAUGHT_CALLBACK.lock().unwrap() = None;
        HAS_UNCAUGHT_CALLBACK.store(false, Ordering::SeqCst);
        crate::Value::Unit
    })
}

pub fn Node_Process_nextTickImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|callback| {
        purust_core::microtasks::current().enqueue(move || {
            callback.unwrap_func1()(crate::Value::Unit);
        });
        crate::Value::Unit
    })))
}

pub fn Node_Process_nextTickCbImpl() -> crate::UnknownType {
    crate::Value::Func2(purust_core::Func2::Shared(Rc::new(|callback, args| {
        purust_core::microtasks::current().enqueue(move || {
            callback.unwrap_func1()(args);
        });
        crate::Value::Unit
    })))
}

// ---------------------------------------------------------------------------
// Arguments, environment and configuration
// ---------------------------------------------------------------------------

pub fn Node_Process_argv() -> crate::UnknownType {
    effect(|| {
        // Node's `process.argv` is `[execPath, scriptPath, ...arguments]`.
        // Native binaries have no separate script path, so the executable
        // occupies the first two entries and user arguments start at index 2,
        // exactly like `node script.js ...`.
        let mut arguments: Vec<crate::UnknownType> = Vec::new();
        let mut raw = std::env::args();
        let executable = raw.next().unwrap_or_default();
        arguments.push(string(&executable));
        arguments.push(string(&executable));
        arguments.extend(raw.map(|argument| string(&argument)));
        crate::mk_array(arguments)
    })
}

pub fn Node_Process_argv0() -> crate::UnknownType {
    effect(|| {
        string(&std::env::args().next().unwrap_or_default())
    })
}

pub fn Node_Process_execArgv() -> crate::UnknownType {
    effect(|| crate::mk_array(Vec::new()))
}

pub fn Node_Process_execPath() -> crate::UnknownType {
    effect(|| {
        let path = std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        string(&path)
    })
}

pub fn Node_Process_cwd() -> crate::UnknownType {
    effect(|| {
        let cwd = std::env::current_dir()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        string(&cwd)
    })
}

pub fn Node_Process_chdirImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|dir| {
        let dir = purust_core::purust_string_to_utf8_lossy(&dir.unwrap_string());
        if let Err(error) = std::env::set_current_dir(&dir) {
            panic!("Node.Process.chdir: {error}");
        }
        crate::Value::Unit
    })))
}

pub fn Node_Process_getEnv() -> crate::UnknownType {
    effect(|| object_value(Rc::new(env_object().snapshot())))
}

pub fn Node_Process_unsafeGetEnv() -> crate::UnknownType {
    effect(|| object_value(env_object()))
}

pub fn Node_Process_setEnvImpl() -> crate::UnknownType {
    crate::Value::Func2(purust_core::Func2::Shared(Rc::new(|key, value| {
        let key = key.unwrap_string();
        let value = value.unwrap_string();
        env_object().insert(key.clone(), crate::Value::String(value.clone()));
        std::env::set_var(
            purust_core::purust_string_to_utf8_lossy(&key),
            purust_core::purust_string_to_utf8_lossy(&value),
        );
        crate::Value::Unit
    })))
}

pub fn Node_Process_unsetEnvImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|key| {
        let key = key.unwrap_string();
        env_object().remove(&key);
        std::env::remove_var(purust_core::purust_string_to_utf8_lossy(&key));
        crate::Value::Unit
    })))
}

pub fn Node_Process_config() -> crate::UnknownType {
    effect(|| object_value(Rc::new(Purs_Foreign_Object::Object::empty())))
}

// ---------------------------------------------------------------------------
// Identity and platform
// ---------------------------------------------------------------------------

pub fn Node_Process_pid() -> i64 {
    std::process::id() as i64
}

pub fn Node_Process_ppid() -> i64 {
    #[cfg(unix)]
    {
        unsafe { libc::getppid() as i64 }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

pub fn Node_Process_platformStr() -> String {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
    .to_owned()
}

pub fn Node_Process_version() -> String {
    "v22.0.0-purust".to_owned()
}

pub fn Node_Process_debugPort() -> i64 {
    9229
}

pub fn Node_Process_uptime() -> crate::UnknownType {
    effect(|| {
        let start = START.get_or_init(std::time::Instant::now);
        crate::mk_number(start.elapsed().as_secs_f64())
    })
}

pub fn Node_Process_getTitle() -> crate::UnknownType {
    effect(|| {
        let title = TITLE.lock().unwrap().clone().unwrap_or_else(|| {
            std::env::args().next().unwrap_or_default()
        });
        string(&title)
    })
}

pub fn Node_Process_setTitleImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|title| {
        let title = title.unwrap_string();
        #[cfg(target_os = "linux")]
        {
            let bytes = purust_core::purust_string_to_utf8_lossy(&title).into_bytes();
            let mut name = bytes[..bytes.len().min(15)].to_vec();
            name.push(0);
            unsafe {
                libc::prctl(libc::PR_SET_NAME, name.as_ptr() as libc::c_ulong, 0, 0, 0);
            }
        }
        *TITLE.lock().unwrap() = Some(title);
        crate::Value::Unit
    })))
}

// ---------------------------------------------------------------------------
// IPC (no channel is ever connected in a native process)
// ---------------------------------------------------------------------------

pub fn Node_Process_channelRefImpl() -> Rc<Purs_Data_Nullable::Nullable> {
    nullable(None)
}

pub fn Node_Process_channelUnrefImpl() -> Rc<Purs_Data_Nullable::Nullable> {
    nullable(None)
}

pub fn Node_Process_disconnectImpl() -> Rc<Purs_Data_Nullable::Nullable> {
    nullable(None)
}

pub fn Node_Process_connected() -> crate::UnknownType {
    effect_bool(false)
}

pub fn Node_Process_sendImpl() -> crate::UnknownType {
    crate::Value::Func2(purust_core::Func2::Shared(Rc::new(|_message, _handle| {
        crate::mk_bool(false)
    })))
}

pub fn Node_Process_sendOptsImpl() -> crate::UnknownType {
    crate::Value::Func3(purust_core::Func3::Shared(Rc::new(
        |_message, _handle, _options| crate::mk_bool(false),
    )))
}

pub fn Node_Process_sendCbImpl() -> crate::UnknownType {
    crate::Value::Func3(purust_core::Func3::Shared(Rc::new(
        |_message, _handle, _callback| crate::mk_bool(false),
    )))
}

pub fn Node_Process_sendOptsCbImpl() -> crate::UnknownType {
    crate::Value::Func4(purust_core::Func4::Shared(Rc::new(
        |_message, _handle, _options, _callback| crate::mk_bool(false),
    )))
}

// ---------------------------------------------------------------------------
// Signals
// ---------------------------------------------------------------------------

pub fn Node_Process_killImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|pid| {
        kill(pid.unwrap_int(), libc::SIGTERM);
        crate::Value::Unit
    })))
}

pub fn Node_Process_killStrImpl() -> crate::UnknownType {
    crate::Value::Func2(purust_core::Func2::Shared(Rc::new(|pid, signal| {
        let name = purust_core::purust_string_to_utf8_lossy(&signal.unwrap_string());
        match signal_number(&name) {
            Some(signal) => {
                kill(pid.unwrap_int(), signal);
            }
            None => panic!("Node.Process.kill: unknown signal '{name}'"),
        }
        crate::Value::Unit
    })))
}

pub fn Node_Process_killIntImpl() -> crate::UnknownType {
    crate::Value::Func2(purust_core::Func2::Shared(Rc::new(|pid, signal| {
        kill(pid.unwrap_int(), signal.unwrap_int() as i32);
        crate::Value::Unit
    })))
}

// ---------------------------------------------------------------------------
// CPU and memory usage
// ---------------------------------------------------------------------------

fn current_cpu_usage() -> (i64, i64) {
    #[cfg(unix)]
    {
        unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
                let user = usage.ru_utime.tv_sec as i64 * 1_000_000 + usage.ru_utime.tv_usec as i64;
                let system = usage.ru_stime.tv_sec as i64 * 1_000_000 + usage.ru_stime.tv_usec as i64;
                return (user, system);
            }
        }
    }
    (0, 0)
}

fn cpu_usage_record(user: i64, system: i64) -> crate::UnknownType {
    record(vec![
        ("user", crate::mk_int(user)),
        ("system", crate::mk_int(system)),
    ])
}

pub fn Node_Process_cpuUsage() -> crate::UnknownType {
    effect(|| {
        let (user, system) = current_cpu_usage();
        cpu_usage_record(user, system)
    })
}

pub fn Node_Process_cpuUsageDiffImpl() -> crate::UnknownType {
    crate::Value::Func1(purust_core::Func1::Shared(Rc::new(|previous| {
        let (user, system) = current_cpu_usage();
        let peek = |name: &str| {
            previous
                .__purust_get_field(name)
                .map(|value| value.unwrap_int())
                .unwrap_or(0)
        };
        cpu_usage_record(user - peek("user"), system - peek("system"))
    })))
}

pub fn Node_Process_memoryUsage() -> crate::UnknownType {
    effect(|| {
        let rss = resident_set_size();
        record(vec![
            ("rss", crate::mk_int(rss)),
            ("heapTotal", crate::mk_int(0)),
            ("heapUsed", crate::mk_int(0)),
            ("external", crate::mk_int(0)),
            ("arrayBuffers", crate::mk_int(0)),
        ])
    })
}

pub fn Node_Process_memoryUsageRss() -> crate::UnknownType {
    effect(|| crate::mk_int(resident_set_size()))
}

pub fn Node_Process_resourceUsage() -> crate::UnknownType {
    effect(|| {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        #[cfg(unix)]
        {
            unsafe {
                libc::getrusage(libc::RUSAGE_SELF, &mut usage);
            }
        }
        record(vec![
            ("userCPUTime", crate::mk_int(usage.ru_utime.tv_sec as i64 * 1_000_000 + usage.ru_utime.tv_usec as i64)),
            ("systemCPUTime", crate::mk_int(usage.ru_stime.tv_sec as i64 * 1_000_000 + usage.ru_stime.tv_usec as i64)),
            ("maxRSS", crate::mk_int(usage.ru_maxrss as i64)),
            ("sharedMemorySize", crate::mk_int(0)),
            ("unsharedDataSize", crate::mk_int(0)),
            ("unsharedStackSize", crate::mk_int(0)),
            ("minorPageFault", crate::mk_int(usage.ru_minflt as i64)),
            ("majorPageFault", crate::mk_int(usage.ru_majflt as i64)),
            ("swappedOut", crate::mk_int(usage.ru_nswap as i64)),
            ("fsRead", crate::mk_int(usage.ru_inblock as i64)),
            ("fsWrite", crate::mk_int(usage.ru_oublock as i64)),
            ("ipcSent", crate::mk_int(usage.ru_msgsnd as i64)),
            ("ipcReceived", crate::mk_int(usage.ru_msgrcv as i64)),
            ("signalsCount", crate::mk_int(usage.ru_nsignals as i64)),
            ("voluntaryContextSwitches", crate::mk_int(usage.ru_nvcsw as i64)),
            ("involuntaryContextSwitches", crate::mk_int(usage.ru_nivcsw as i64)),
        ])
    })
}

// ---------------------------------------------------------------------------
// Standard streams
// ---------------------------------------------------------------------------

pub fn Node_Process_stdin() -> crate::UnknownType {
    Purs_Node_Stream::purust_stream_box(stdin_stream())
}

pub fn Node_Process_stdout() -> crate::UnknownType {
    Purs_Node_Stream::purust_stream_box(stdio_stream(1, &STDOUT))
}

pub fn Node_Process_stderr() -> crate::UnknownType {
    Purs_Node_Stream::purust_stream_box(stdio_stream(2, &STDERR))
}

pub fn Node_Process_stdinIsTTY() -> bool {
    fd_is_tty(0)
}

pub fn Node_Process_stdoutIsTTY() -> bool {
    fd_is_tty(1)
}

pub fn Node_Process_stderrIsTTY() -> bool {
    fd_is_tty(2)
}

// Keep the emitter boxing helper referenced even when no accessor uses it in a
// trimmed build; `process` is always an emitter.
#[allow(dead_code)]
fn process_handle() -> crate::UnknownType {
    purust_emitter_box(process_emitter())
}
