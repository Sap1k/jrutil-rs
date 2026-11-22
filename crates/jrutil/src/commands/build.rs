//! `jrutil build --spec <build.toml>`: compile the packages a build specification names.

use std::path::Path;
use std::process::ExitCode;

use crate::pipeline;
use crate::spec::BuildSpec;

/// `use_cache`: whether caches named by the specification are read and written.
pub fn run(spec_path: &Path, use_cache: bool) -> ExitCode {
    let spec = match BuildSpec::read(spec_path) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Some(czptt) = &spec.czptt
        && let Err(error) = pipeline::czptt(czptt, use_cache)
    {
        eprintln!("error: czptt: {error}");
        return ExitCode::FAILURE;
    }
    if let Some(jdf) = &spec.jdf
        && let Err(error) = pipeline::jdf(jdf, use_cache)
    {
        eprintln!("error: jdf: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
