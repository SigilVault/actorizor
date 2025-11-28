use quote::ToTokens;
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

/// Use `rustfmt` to pretty-print the tokens.
#[allow(dead_code)]
pub fn pretty_print(tokens: impl ToTokens) -> Result<String, Box<dyn std::error::Error>> {
    let tokens = tokens.into_token_stream().to_string();

    let mut child = Command::new("rustfmt")
        .arg("--edition=2024")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Unable to start `rustfmt`. Is it installed?");

    let mut stdin = child.stdin.take().unwrap();
    write!(stdin, "{tokens}")?;
    stdin.flush()?;
    drop(stdin);

    let Output {
        status,
        stdout,
        stderr,
    } = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);

    if !status.success() {
        eprintln!("---- Stdout ----");
        eprintln!("{stdout}");
        eprintln!("---- Stderr ----");
        eprintln!("{stderr}");
        let code = status.code();
        match code {
            Some(code) => panic!("The `rustfmt` command failed with return code {code}"),
            None => panic!("The `rustfmt` command failed"),
        }
    }

    Ok(stdout.into())
}
