//! The project doctor's Terraform checks: Terraform itself, which the
//! tasks, `terraform fmt` and `terraform validate` run, and terraform-ls,
//! the language server, which has a fix row installing HashiCorp's
//! release for this machine. Terraform has none: HashiCorp, Homebrew,
//! winget and `tfenv` each say how, and which version is the project's
//! to choose.

use fenix_lang_kit::{Fix, Program};

use crate::project::Project;
use crate::tools::{hints, Platform};

/// The oldest Terraform whose `-chdir`, `validate -json` and `fmt -` the
/// pack runs.
pub const OLDEST: &str = "1.0";

/// `name`, looked for where the pack installs it and the managers do.
fn program(name: &str, label: &str) -> Program {
    hints(name).into_iter().fold(Program::new(name, label), Program::hint).version_args(["version"])
}

/// The checks for `project`. Terraform is needed in a Terraform project
/// and only noticed in any other; the server is optional: without it
/// Fenix still colours and formats.
pub fn programs(project: &Project, platform: &Platform) -> Vec<Program> {
    let terraform = program("terraform", "Terraform").at_least(OLDEST);
    let terraform = if project.is_terraform() { terraform } else { terraform.optional() };
    let mut ls = program("terraform-ls", "terraform-ls").optional();
    if platform.installs().iter().any(|i| i.name == "terraform-ls") {
        ls = ls.fix("Install terraform-ls", "terraform.install.terraform-ls", Fix::Install("terraform-ls".into()));
    }
    vec![terraform, ls]
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::Health;
    use fenix_lang_kit::Output;

    fn said(stdout: &str) -> Output {
        Output { code: Some(0), stdout: stdout.into(), stderr: String::new() }
    }

    fn project() -> Project {
        Project { module: Some(".".into()) }
    }

    #[test]
    fn terraform_is_asked_its_version_and_needed_in_a_terraform_project() {
        let linux = Platform::new("linux", "x86_64");
        let p = programs(&project(), &linux);
        assert_eq!(p.iter().map(|p| (p.label.as_str(), p.required)).collect::<Vec<_>>(), [("Terraform", true), ("terraform-ls", false)]);
        assert_eq!(p[0].version_args, ["version"]);
        assert_eq!(p[0].hints[0], "/tools/terraform");
        assert!(p[0].fix.is_none(), "HashiCorp's to install, and the version the project's to choose");

        // `terraform version` as 1.17.2 prints it: its version, and the platform.
        let c = p[0].evaluate(Some("/usr/bin/terraform"), Some(&said(include_str!("samples/terraform-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "1.17.2 (/usr/bin/terraform)"));
        let old = p[0].evaluate(Some("/usr/bin/terraform"), Some(&said("Terraform v0.15.5\non linux_amd64\n")));
        assert_eq!(old.health, Health::Bad, "{}", old.detail);
        assert_eq!(p[0].evaluate(None, None).health, Health::Bad, "nothing runs without it");

        let other = programs(&Project::default(), &linux);
        assert_eq!(other[0].evaluate(None, None).health, Health::Warn, "a project that isn't Terraform's isn't failed for it");
    }

    #[test]
    fn terraform_ls_has_a_fix_row_where_there_is_a_release() {
        let p = programs(&project(), &Platform::new("linux", "x86_64"));
        assert_eq!(p[1].fix.as_ref().map(|f| f.1.as_str()), Some("terraform.install.terraform-ls"));
        let c = p[1].evaluate(Some("/tools/terraform-ls/terraform-ls"), Some(&said(include_str!("samples/terraform-ls-version.txt"))));
        assert_eq!((c.health, c.detail.as_str()), (Health::Ok, "0.39.1 (/tools/terraform-ls/terraform-ls)"));
        let missing = p[1].evaluate(None, None);
        assert_eq!(missing.health, Health::Warn, "Fenix works without it");
        assert_eq!(missing.fix.map(|f| f.1), Some("terraform.install.terraform-ls".into()));

        let none = programs(&project(), &Platform::new("linux", "riscv64"));
        assert!(none[1].fix.is_none(), "no release for this machine");
    }
}
