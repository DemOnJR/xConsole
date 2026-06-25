//! Thin entry point for the headless benchmark/eval harness. Built as a separate
//! binary so it never starts a webview and a running xConsole can't lock its exe.
//! All logic lives in `xconsole_lib::bench`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    xconsole_lib::bench::run(&args);
}
