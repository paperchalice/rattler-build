use std::fmt::Write as _;
use std::path::Path;

use indexmap::IndexMap;
use rattler_shell::shell::{self, Shell};

use super::{CommandSpec, ShellDialect};
use crate::{ExecutionContext, PrefixLayout};

pub(crate) struct PowerShellDialect;

impl ShellDialect for PowerShellDialect {
    fn shell(&self) -> shell::ShellEnum {
        shell::PowerShell::default().into()
    }

    fn default_interpreter(&self) -> &'static str {
        "powershell"
    }

    fn preamble(&self, activation_script_path: &Path) -> String {
        format!(
            r#"
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ([string]::IsNullOrEmpty($Env:CONDA_BUILD)) {{
    . {}
}}

foreach ($envVar in Get-ChildItem Env:) {{
    if (-not (Test-Path -Path Variable:$($envVar.Name))) {{
        Set-Variable -Name $envVar.Name -Value $envVar.Value
    }}
}}

"#,
            activation_script_path.to_string_lossy()
        )
    }

    fn command_to_run_script<'a>(
        &self,
        build_script_path: &Path,
        _context: &ExecutionContext,
    ) -> CommandSpec {
        CommandSpec::new(
            "pwsh",
            [
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                build_script_path.to_string_lossy().into_owned(),
            ],
        )
    }

    fn replacements_template(&self) -> &'static str {
        "$((var))"
    }

    fn supports_sandbox(&self) -> bool {
        false
    }

    fn scope_section(
        &self,
        label: Option<&str>,
        env: &IndexMap<String, String>,
        cwd: Option<&Path>,
        body: &str,
    ) -> Result<String, std::io::Error> {
        let shell = shell::PowerShell::default();
        let mut out = String::new();
        if let Some(label) = label {
            let _ = writeln!(out, "# === {label} ===");
        }
        out.push_str("& {\n");
        for (key, value) in env {
            shell
                .set_env_var(&mut out, key, value)
                .map_err(std::io::Error::other)?;
        }
        if let Some(cwd) = cwd {
            let cwd = super::quote_arg(&self.shell(), &cwd.to_string_lossy());
            let _ = writeln!(out, "Set-Location -Path {cwd}");
        }
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("}\n");
        Ok(out)
    }

    fn debug_info(&self, work_dir: &Path, context: &ExecutionContext) -> String {
        let mut output = String::new();

        output.push_str("\nScript execution failed.\n\n");
        output.push_str(&format!("  Work directory: {}\n", work_dir.display()));
        output.push_str(&format!("  Prefix: {}\n", context.host().path().display()));

        if context.layout() == PrefixLayout::Separate {
            output.push_str(&format!(
                "  Build prefix: {}\n",
                context.build().path().display()
            ));
        } else {
            output.push_str("  Build prefix: None\n");
        }

        output.push_str("\nTo run the script manually, use the following command:\n");
        output.push_str(&format!("  cd {:?} && ./conda_build.ps1\n\n", work_dir));
        output.push_str("To run commands interactively in the build environment:\n");
        output.push_str(&format!("  cd {:?} && call build_env.ps1", work_dir));

        output
    }
}
