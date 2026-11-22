//! `jrutil build --spec <build.toml>`: compile the packages a build specification names.

use std::path::Path;
use std::process::ExitCode;

use crate::pipeline;
use crate::spec::BuildSpec;

pub fn run(spec_path: &Path) -> ExitCode {
    let spec = match BuildSpec::read(spec_path) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Some(czptt) = &spec.czptt
        && let Err(error) = pipeline::czptt(czptt)
    {
        eprintln!("error: czptt: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
