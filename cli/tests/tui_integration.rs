use std::process::Command;

#[test]
fn tui_help_exposes_runtime_options_and_main_key_concepts() {
    let output = Command::new(env!("CARGO_BIN_EXE_harpe-tui"))
        .arg("--help")
        .output()
        .expect("run harpe-tui --help");

    assert!(
        output.status.success(),
        "harpe-tui --help failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("help output is utf8");
    assert!(stdout.contains("Terminal roleplay cockpit"));
    assert!(stdout.contains("--addr"));
    assert!(stdout.contains("--user-id"));
    assert!(stdout.contains("--game-id"));
    assert!(stdout.contains("--session-id"));
    assert!(stdout.contains("--model"));
}
