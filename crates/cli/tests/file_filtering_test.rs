use insta::assert_snapshot;
use rexpect::session::spawn_command;
use std::fs::read_to_string;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::str::from_utf8;

#[test]
fn it_filters_yaml_files() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--path *createWithList tests/resources/petstore.yaml".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_filters_json_files() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--tag item tests/resources/user-reference.json".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_reports_parsing_errors() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--tag item tests/resources/invalid-content".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_reports_io_errors() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(bin_path, "--tag item tests/resources/not_found".into());

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());

    Ok(())
}

#[test]
fn it_filters_openapiv3_1_files() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--path *createWithList tests/resources/full-coverage-3.1.yaml".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_filters_webhooks_by_name_flag() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--webhook userCreated tests/resources/full-coverage-3.1.yaml".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_filters_a_dot_yml_extensioned_file() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(bin_path, "tests/resources/minimal-api.yml".into());

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_follows_a_deep_ref_chain_from_a_json_file() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let cmd = define_command(
        bin_path,
        "--tag chain tests/resources/deep-ref-chain.json".into(),
    );

    let mut process = spawn_command(cmd, Some(30000))?;

    let result = process.exp_eof()?;

    assert_snapshot!(result.trim_end());
    Ok(())
}

#[test]
fn it_handled_piped_input_with_explicit_pipe_marker_yaml()
-> Result<(), Box<dyn std::error::Error>> {
    // Read the test file
    let contents = read_to_string("tests/resources/petstore.yaml")?;

    // Create the command with configured stdin
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("openapiv3-filter"))
        .arg("--path")
        .arg("/user")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // Get stdin handle and write to it
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(contents.as_bytes())?;
        // stdin is automatically closed when dropped here
    }

    // Wait for the command to complete and get output
    let output = child.wait_with_output()?;

    // Assert the command succeeded
    assert!(output.status.success());
    // Convert stdout to string
    let stdout_str = from_utf8(&output.stdout)?.to_string();
    assert_snapshot!(stdout_str);

    Ok(())
}

#[test]
fn it_handled_piped_input_without_explicit_pipe_marker_yaml()
-> Result<(), Box<dyn std::error::Error>> {
    // Read the test file
    let contents = read_to_string("tests/resources/petstore.yaml")?;

    // Create the command with configured stdin
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("openapiv3-filter"))
        .arg("--path")
        .arg("/user")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // Get stdin handle and write to it
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(contents.as_bytes())?;
        // stdin is automatically closed when dropped here
    }

    // Wait for the command to complete and get output
    let output = child.wait_with_output()?;

    // Assert the command succeeded
    assert!(output.status.success());
    // Convert stdout to string
    let stdout_str = from_utf8(&output.stdout)?.to_string();
    assert_snapshot!(stdout_str);

    Ok(())
}

#[test]
fn it_handled_piped_input_without_explicit_pipe_marker_without_filtering_yaml()
-> Result<(), Box<dyn std::error::Error>> {
    // Read the test file
    let contents = read_to_string("tests/resources/user-reference.yaml")?;

    // Create the command with configured stdin
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("openapiv3-filter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // Get stdin handle and write to it
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(contents.as_bytes())?;
        // stdin is automatically closed when dropped here
    }

    // Wait for the command to complete and get output
    let output = child.wait_with_output()?;

    // Assert the command succeeded
    assert!(output.status.success());
    // Convert stdout to string
    let stdout_str = from_utf8(&output.stdout)?.to_string();
    assert_snapshot!(stdout_str);

    Ok(())
}

#[test]
fn it_handled_piped_input_without_explicit_pipe_marker_without_filtering_json()
-> Result<(), Box<dyn std::error::Error>> {
    // Read the test file
    let contents = read_to_string("tests/resources/user-reference.json")?;

    // Create the command with configured stdin
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("openapiv3-filter"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    // Get stdin handle and write to it
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(contents.as_bytes())?;
        // stdin is automatically closed when dropped here
    }

    // Wait for the command to complete and get output
    let output = child.wait_with_output()?;

    // Assert the command succeeded
    assert!(output.status.success());
    // Convert stdout to string
    let stdout_str = from_utf8(&output.stdout)?.to_string();
    assert_snapshot!(stdout_str);

    Ok(())
}

#[test]
fn it_keeps_components_whose_names_contain_dots_slashes_or_tildes() -> Result<(), Box<dyn std::error::Error>> {
    let bin_path = assert_cmd::cargo::cargo_bin("openapiv3-filter");

    let output = Command::new(bin_path)
        .args(["--path", "/auth", "tests/resources/special-component-names.json"])
        .output()?;
    assert!(output.status.success());

    let document: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut schemas: Vec<&str> = document["components"]["schemas"]
        .as_object()
        .expect("schemas are kept")
        .keys()
        .map(String::as_str)
        .collect();
    schemas.sort();
    assert_eq!(
        schemas,
        vec!["Foo Bar", "a/b", "billing.feedback_option", "issuing.authorization", "til~de"]
    );
    assert!(document["components"]["responses"]["not.found"].is_object());
    Ok(())
}

fn define_command(bin_path: PathBuf, command: String) -> Command {
    let mut cmd = Command::new(bin_path);
    cmd.args(command.split(" "));
    cmd
}
