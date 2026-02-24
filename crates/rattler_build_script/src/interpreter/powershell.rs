use std::path::PathBuf;
use std::process::Command;

use rattler_conda_types::Platform;
use rattler_shell::{
    activation::{ActivationVariables, Activator, PathModificationBehavior},
    shell,
};

use crate::execution::ExecutionArgs;

use super::{BashInterpreter, CmdExeInterpreter, Interpreter, InterpreterError, find_interpreter};

pub(crate) struct PowerShellInterpreter;

const POWERSHELL_PREAMBLE: &str = r#"
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

foreach ($envVar in Get-ChildItem Env:) {
    if (-not (Test-Path -Path Variable:$($envVar.Name))) {
        Set-Variable -Name $envVar.Name -Value $envVar.Value
    }
}

"#;

const POWERSHELL_POSTAMBLE: &str = r#"
if (Get-Command 'upx' -ErrorAction SilentlyContinue) {
    $files = Get-ChildItem -Path $LIBRARY_PREFIX -Recurse -Include *.exe, *.dll -Attributes !ReparsePoint
    if ($files) {
        upx -9 $files
    }
}

tree $PREFIX /F

"#;

/// Check if pwsh (PowerShell 7+) is available and determine its version.
/// Returns (shell_command, is_new_enough).
fn detect_powershell() -> (&'static str, bool) {
    let result: Option<bool> = which::which("pwsh").ok().and_then(|_| {
        let out = String::from_utf8(Command::new("pwsh").arg("-v").output().ok()?.stdout).ok()?;
        let ver = out
            .trim()
            .split(' ')
            .next_back()?
            .split('.')
            .collect::<Vec<&str>>();
        if ver.len() < 2 {
            return None;
        }

        let major = ver[0].parse::<i32>().ok()?;
        let minor = ver[1].parse::<i32>().ok()?;
        Some(major > 7 || (major == 7 && minor >= 4))
    });

    match result {
        Some(new_enough) => ("pwsh", new_enough),
        None => ("powershell", false),
    }
}

// PowerShell interpreter: writes a .ps1 script then delegates to cmd.exe (Windows) or bash (Unix)
// to run it via the pwsh/powershell command.
impl Interpreter for PowerShellInterpreter {
    async fn run(&self, args: ExecutionArgs) -> Result<(), InterpreterError> {
        let (shell_cmd, new_enough) = detect_powershell();

        if !new_enough {
            tracing::warn!(
                "rattler-build requires PowerShell 7.4+, \
                 otherwise it will skip native command errors!"
            );
        }

        let mut shell_script =
            shell::ShellScript::new(shell::PowerShell::default(), Platform::current());
        let host_prefix_activator = Activator::from_path(
            &args.run_prefix,
            shell::PowerShell::default(),
            args.execution_platform,
        )
        .unwrap();
        let vars = ActivationVariables {
            path_modification_behavior: PathModificationBehavior::Append,
            ..Default::default()
        };
        let host_activation = host_prefix_activator.activation(vars.clone()).unwrap();
        if let Some(build_prefix) = &args.build_prefix {
            let build_prefix_activator = Activator::from_path(
                build_prefix,
                shell::PowerShell::default(),
                args.execution_platform,
            )
            .unwrap();

            let build_activation = build_prefix_activator.activation(vars.clone()).unwrap();
            shell_script.append_script(&host_activation.script);
            shell_script.append_script(&build_activation.script);
        } else {
            shell_script.append_script(&host_activation.script);
        }
        let ps1_script = args.work_dir.join("conda_build_script.ps1");
        let contents = shell_script.contents().unwrap()
            + POWERSHELL_PREAMBLE
            + args.script.script()
            + POWERSHELL_POSTAMBLE;
        tokio::fs::write(&ps1_script, contents).await?;

        let args = ExecutionArgs {
            script: crate::execution::ResolvedScriptContents::Inline(format!(
                "{} -NoLogo -NoProfile {:?}",
                shell_cmd, ps1_script
            )),
            ..args
        };

        if cfg!(windows) {
            CmdExeInterpreter.run(args).await
        } else {
            BashInterpreter.run(args).await
        }
    }

    async fn find_interpreter(
        &self,
        build_prefix: Option<&PathBuf>,
        platform: &Platform,
    ) -> Result<Option<PathBuf>, which::Error> {
        find_interpreter("pwsh", build_prefix, platform)
    }
}
