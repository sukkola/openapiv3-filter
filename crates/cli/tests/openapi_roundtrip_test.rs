//! Regression guard for the move away from `openapiv3`-struct-typed filtering (see design.md,
//! "Risks" section): confirms representative filtered 3.0 outputs still deserialize via the
//! `openapiv3` crate, without making that crate a runtime dependency of the CLI.

use assert_cmd::cargo::cargo_bin;
use openapiv3::OpenAPI;
use std::process::Command;

#[test]
fn filtered_3_0_output_round_trips_through_openapiv3() {
    let output = Command::new(cargo_bin("openapiv3-filter"))
        .args(["--path", "/pet/{petId}", "tests/resources/petstore.yaml"])
        .output()
        .expect("failed to run openapiv3-filter");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout was not valid utf-8");

    let parsed: Result<OpenAPI, _> = openapiv3_filter_core::yaml::from_str(&stdout);
    assert!(
        parsed.is_ok(),
        "filtered 3.0 output failed to deserialize as a structurally valid OpenAPI 3.0 document: {:?}",
        parsed.err()
    );
}
