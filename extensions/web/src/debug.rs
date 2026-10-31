//! js-debug, VS Code's JavaScript debugger, as the debug adapter: its
//! DAP server (`dapDebugServer.js`) from the release's tarball, which is
//! the same on every machine, installed into the tools folder and run by
//! Node. It speaks DAP over a socket, not stdio, so it's given with
//! `debug-adapter-connect` (0.8.4): Fenix starts it on a free port
//! (`${port}`) and connects.

use fenix_extension_api::serde_json::json;
use fenix_extension_api::{Command, LaunchConfig};
use fenix_lang_kit::Install;

use crate::project::Project;

/// The release installed, and its tarball's SHA-256.
pub const VERSION: &str = "1.140.0";
pub const SHA256: &str = "27dab92937ec1ab35821ae955aac867544fe06a1b6307229049f2d789af10968";

/// Its folder in the tools folder, and its name in `web.install.js-debug`.
pub const NAME: &str = "js-debug";

/// Fenix's names for what it debugs.
pub const LANGUAGES: &[&str] = &["javascript", "typescript", "tsx"];

pub fn url() -> String {
    format!("https://github.com/microsoft/vscode-js-debug/releases/download/v{VERSION}/js-debug-dap-v{VERSION}.tar.gz")
}

/// The tarball's `js-debug/` taken off: `/tools/js-debug/src/dapDebugServer.js`.
pub fn install() -> Install {
    Install::new(NAME, &url()).label(&format!("js-debug {VERSION}")).sha256(SHA256).strip(1).program("src/dapDebugServer.js")
}

/// The adapter: `node dapDebugServer.js <port> 127.0.0.1`, by the Node
/// found.
pub fn adapter(node: Command) -> Command {
    node.args(["${tools}/js-debug/src/dapDebugServer.js", "${port}", "127.0.0.1"])
}

/// A launch for each of `package.json`'s scripts (`launches-set`, 0.8.6),
/// run by the project's manager with the debugger on each Node it starts
/// (`npm run dev`, `pnpm run test`): a launch file can't know them, nor the
/// manager.
pub fn launches(project: &Project) -> Vec<LaunchConfig> {
    if !project.package_json {
        return Vec::new();
    }
    let pm = project.manager.program();
    crate::tasks::scripts(project)
        .into_iter()
        .map(|s| {
            LaunchConfig::launch(&format!("Node: {pm} run {s}"), LANGUAGES).cwd("${project}").attributes(&json!({
                "type": "pwa-node",
                "runtimeExecutable": pm,
                "runtimeArgs": ["run", s],
                "autoAttachChildProcesses": true,
                "skipFiles": ["<node_internals>/**"],
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    #[test]
    fn js_debug_is_the_platform_independent_tarball_run_by_node() {
        let i = install();
        assert_eq!(i.url, "https://github.com/microsoft/vscode-js-debug/releases/download/v1.140.0/js-debug-dap-v1.140.0.tar.gz");
        assert_eq!((i.kind, i.strip, i.program.as_str(), i.sha256.as_deref()), (fenix_lang_kit::archive::Kind::TarGz, 1, "src/dapDebugServer.js", Some(SHA256)));
        assert!(i.uses.is_empty(), "it's set with debug-adapter-connect, not by the kit");
        assert_eq!(line(&adapter(Command::new("/n/node"))), "/n/node ${tools}/js-debug/src/dapDebugServer.js ${port} 127.0.0.1");
    }

    #[test]
    fn each_script_is_a_launch_by_the_projects_manager() {
        let project = Project::parse(r#"{"scripts": {"test": "vitest", "dev": "vite", "predev": "x"}}"#, &["pnpm-lock.yaml"]).unwrap();
        let l = launches(&project);
        assert_eq!(l.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Node: pnpm run dev", "Node: pnpm run test"], "predev runs with dev");
        let attrs: fenix_extension_api::serde_json::Value = fenix_extension_api::serde_json::from_str(&l[0].attributes).unwrap();
        assert_eq!((attrs["runtimeExecutable"].as_str(), attrs["runtimeArgs"][1].as_str()), (Some("pnpm"), Some("dev")));
        assert_eq!(l[0].program, None, "the manager runs, no program");
        assert!(launches(&Project::default()).is_empty());
    }
}
