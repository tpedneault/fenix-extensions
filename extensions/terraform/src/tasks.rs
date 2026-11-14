//! The tasks (`SPC p t`), each run in the root module's folder:
//!
//! | Task | Runs |
//! |---|---|
//! | `init` | `terraform init` |
//! | `validate` | `terraform validate` |
//! | `fmt -check` | `terraform fmt -check -diff -recursive`: what isn't formatted, and how it would change |
//! | `plan` | `terraform plan -out=tfplan`: what would change, kept as a file |
//! | `apply saved plan` | `terraform apply tfplan`: **changes real infrastructure** |
//!
//! A task's stdin is closed, so Terraform can't ask "Do you want to
//! perform these actions?" and `terraform apply` alone would fail. The pack
//! doesn't answer for you with `-auto-approve` either: `apply` applies
//! the plan file that `plan` wrote, and Terraform refuses one that's
//! stale. So there's always a plan you could have read before it, applying
//! twice is refused, and it's last in the picker; it has no key of its own.
//! Nothing is run without `-input=false`: a variable without a value is an
//! error, not a prompt nothing could answer.

use fenix_lang_kit::Tasks;

use crate::project::Project;
use crate::tools::Tools;

/// The plan file `plan` writes and `apply saved plan` reads, in the root
/// module's folder. `.gitignore` it: a plan can hold secrets.
pub const PLAN: &str = "tfplan";

/// The apply task's name, which says what it does.
pub const APPLY: &str = "apply saved plan";

/// The tasks for `project` in folder `root`; none when it isn't a
/// Terraform project. The project's `.env` goes to each (`TF_VAR_name`
/// sets a variable, `AWS_PROFILE` and the like a provider's).
pub fn tasks(project: &Project, tools: &Tools, root: &str) -> Tasks {
    let mut t = Tasks::default();
    if !project.is_terraform() {
        return t;
    }
    let terraform = || {
        let cmd = tools.terraform();
        match project.chdir() {
            Some(chdir) => cmd.arg(chdir),
            None => cmd,
        }
    };
    t.add("init", terraform().args(["init", "-input=false", "-no-color"]));
    t.add("validate", terraform().args(["validate", "-no-color"]));
    t.add("fmt -check", terraform().args(["fmt", "-check", "-diff", "-recursive", "-no-color"]));
    t.add("plan", terraform().args(["plan", "-input=false", "-no-color", &format!("-out={PLAN}")]));
    t.add(APPLY, terraform().args(["apply", "-input=false", "-no-color", PLAN]));
    t.env_file(&format!("{root}/.env"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;
    use crate::tests::line;

    fn table(t: &Tasks) -> Vec<(String, String)> {
        t.resolved().iter().map(|t| (t.name.clone(), line(&t.cmd))).collect()
    }

    fn rows(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn a_project_has_init_validate_format_check_plan_and_apply() {
        let project = Project { module: Some(".".into()) };
        let tools = Tools { terraform: Some("/t/terraform".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere")),
            rows(&[
                ("init", "/t/terraform init -input=false -no-color"),
                ("validate", "/t/terraform validate -no-color"),
                ("fmt -check", "/t/terraform fmt -check -diff -recursive -no-color"),
                ("plan", "/t/terraform plan -input=false -no-color -out=tfplan"),
                ("apply saved plan", "/t/terraform apply -input=false -no-color tfplan"),
            ])
        );
    }

    #[test]
    fn apply_is_last_says_what_it_does_and_never_approves_for_you() {
        let project = Project { module: Some(".".into()) };
        let t = tasks(&project, &Tools::default(), "/nowhere");
        let names: Vec<&str> = t.tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.last(), Some(&APPLY), "last, after the plan it applies");
        assert!(!names.contains(&"apply") && !names.contains(&"build") && !names.contains(&"test") && !names.contains(&"run"), "no name a key or a default would take");
        for task in &t.tasks {
            let args = task.cmd.args.join(" ");
            assert!(!args.contains("auto-approve") && !args.contains("destroy"), "{}: {args}", task.name);
        }
        let apply = t.tasks.iter().find(|t| t.name == APPLY).unwrap();
        assert_eq!(apply.cmd.args.last().map(String::as_str), Some(PLAN), "the saved plan, not a fresh one");
        assert!(t.tasks.iter().all(|t| t.cmd.args.iter().any(|a| a == "-input=false" || a == "-check") || t.name == "validate"), "none waits for an answer");
    }

    #[test]
    fn a_root_module_in_a_folder_is_run_there_with_the_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "TF_VAR_region=eu-west-1\nAWS_PROFILE=orbit\n").unwrap();
        let project = Project { module: Some("infra".into()) };
        let t = tasks(&project, &Tools::default(), &dir.to_string_lossy());
        assert_eq!(table(&t)[0], ("init".to_string(), "terraform -chdir=infra init -input=false -no-color".to_string()));
        assert_eq!(table(&t)[4].1, "terraform -chdir=infra apply -input=false -no-color tfplan");
        let env = [("TF_VAR_region".to_string(), "eu-west-1".to_string()), ("AWS_PROFILE".to_string(), "orbit".to_string())];
        assert!(t.resolved().iter().all(|t| t.cmd.env == env));
        assert!(tasks(&Project::default(), &Tools::default(), "/nowhere").tasks.is_empty(), "not a Terraform project");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
