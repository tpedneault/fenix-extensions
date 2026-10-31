//! Debugging a CMake target: each program CMake's file API says the build
//! makes is a launch the pack gives as it runs (`launches-set`, 0.8.6),
//! the target built first (`preLaunchTask`, its `build <target>` task);
//! and `${cmakeTarget}`, a variable filled in with them
//! (`launch-variable-set`), picked as the session starts, for the launch
//! file that debugs whichever you pick.

use fenix_extension_api::{LaunchConfig, PickerItem};

use crate::fileapi::Executable;

/// The variable the programs fill in.
pub const VARIABLE: &str = "cmakeTarget";

/// Fenix's names for the languages.
const LANGUAGES: &[&str] = &["c", "cpp"];

/// The task that builds program `name` (`tasks.rs`).
pub fn build_task(name: &str) -> String {
    format!("build {name}")
}

/// A launch for each program, built first when one of `tasks` (their
/// names) builds it.
pub fn launches(programs: &[Executable], tasks: &[String]) -> Vec<LaunchConfig> {
    programs
        .iter()
        .map(|p| {
            let l = LaunchConfig::launch(&format!("CMake: {}", p.name), LANGUAGES).program(&p.program).cwd("${project}");
            let task = build_task(&p.name);
            if tasks.contains(&task) {
                l.pre_launch_task(&task)
            } else {
                l
            }
        })
        .collect()
}

/// `${cmakeTarget}`'s choices: each program, by its target's name.
pub fn choices(programs: &[Executable]) -> Vec<PickerItem> {
    programs.iter().map(|p| PickerItem::new(p.name.clone(), p.program.clone()).detail(p.program.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fenix_extension_api::LaunchRequest;

    fn exe(name: &str, program: &str) -> Executable {
        Executable { name: name.into(), program: program.into(), source: "/home/u/orbit".into() }
    }

    #[test]
    fn each_program_is_a_launch_built_first() {
        let programs = [exe("orbit_app", "/home/u/orbit/build/debug/bin/orbit"), exe("orbit_tests", "/home/u/orbit/build/debug/orbit_tests")];
        let l = launches(&programs, &["build".into(), "build orbit_app".into()]);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].name.as_str(), l[0].request, l[0].program.as_deref(), l[0].cwd.as_deref()), ("CMake: orbit_app", LaunchRequest::Launch, Some("/home/u/orbit/build/debug/bin/orbit"), Some("${project}")));
        assert_eq!(l[0].languages, ["c", "cpp"]);
        assert_eq!(l[0].pre_launch_task.as_deref(), Some("build orbit_app"));
        assert_eq!(l[1].pre_launch_task, None, "no task to build it");
        let c = choices(&programs);
        assert_eq!((c[1].label.as_str(), c[1].value.as_str()), ("orbit_tests", "/home/u/orbit/build/debug/orbit_tests"));
    }
}
