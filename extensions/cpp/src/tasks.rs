//! The tasks (`SPC p t`): configure, build, test (CTest) and clean, from
//! the project's CMake presets when it has them, else in `build`; the
//! same for each other preset; `build <target>` and `run <target>` for
//! each program CMake said the build makes; and `conan install` for a Conan project. A task
//! of the same name in `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::fileapi::Executable;
use crate::presets::Configure;
use crate::project::Project;
use crate::tools::Tools;

/// `cmake --preset <name>`, telling CMake to write `compile_commands.json`
/// when the preset doesn't.
fn configure_preset(tools: &Tools, c: &Configure) -> Command {
    let cmd = tools.cmake().args(["--preset", &c.name]);
    if c.exports_commands() || c.visual_studio() {
        cmd
    } else {
        cmd.arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
    }
}

fn build_preset(project: &Project, tools: &Tools, c: &Configure) -> Option<Command> {
    let presets = project.presets.as_ref()?;
    match (presets.build_for(&c.name), &c.binary_dir) {
        (Some(b), _) => Some(tools.cmake().args(["--build", "--preset", &b.name])),
        (None, Some(dir)) => Some(tools.cmake().args(["--build", dir])),
        (None, None) => None,
    }
}

fn test_preset(project: &Project, tools: &Tools, c: &Configure) -> Option<Command> {
    let presets = project.presets.as_ref()?;
    match (presets.test_for(&c.name), &c.binary_dir) {
        (Some(t), _) => Some(tools.ctest().args(["--preset", &t.name])),
        (None, Some(dir)) => Some(tools.ctest().args(["--test-dir", dir, "--output-on-failure"])),
        (None, None) => None,
    }
}

/// Configuring without presets, in `build`: Ninja when it's there, and
/// vcpkg's or Conan's toolchain when the project uses them.
fn configure_plain(project: &Project, tools: &Tools) -> Command {
    let mut cmd = tools.cmake().args(["-S", ".", "-B", "build", "-DCMAKE_BUILD_TYPE=Debug", "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"]);
    if tools.ninja.is_some() {
        cmd = cmd.args(["-G", "Ninja"]);
    }
    if let (Some(_), Some(root)) = (&project.vcpkg, tools.vcpkg_root()) {
        cmd = cmd.arg(format!("-DCMAKE_TOOLCHAIN_FILE={root}/scripts/buildsystems/vcpkg.cmake"));
    } else if project.conan.as_ref().is_some_and(|c| !c.layout) {
        // `conan install --output-folder=build` wrote it there.
        cmd = cmd.arg("-DCMAKE_TOOLCHAIN_FILE=conan_toolchain.cmake");
    }
    cmd
}

/// `conan install`: into `build` without a layout (the configure task
/// reads its toolchain there), else where the layout says, which writes
/// CMake presets for the build.
fn conan_install(project: &Project, tools: &Tools) -> Option<Command> {
    let conan = project.conan.as_ref()?;
    let cmd = tools.conan().args(["install", ".", "--build=missing", "-s", "build_type=Debug"]);
    Some(if conan.layout { cmd } else { cmd.arg("--output-folder=build") })
}

/// The tasks for `project`, with what's on this machine; `preset` is
/// `cpp.preset`, `programs` what the build makes.
pub fn tasks(project: &Project, tools: &Tools, preset: Option<&str>, programs: &[Executable]) -> Tasks {
    let mut t = Tasks::default();
    if !project.is_cpp() {
        return t;
    }
    if let Some(conan) = conan_install(project, tools) {
        t.add("conan install", conan);
    }
    let mut build = None;
    match project.preset(preset) {
        Some(main) => {
            t.add("configure", configure_preset(tools, main));
            if let Some(b) = build_preset(project, tools, main) {
                t.add("clean", b.clone().args(["--target", "clean"]));
                t.add("build", b.clone());
                build = Some(b);
            }
            if let Some(test) = test_preset(project, tools, main) {
                t.add("test", test);
            }
            for c in project.presets.iter().flat_map(|p| p.visible()).filter(|c| c.name != main.name) {
                t.add(&format!("configure {}", c.name), configure_preset(tools, c));
                if let Some(b) = build_preset(project, tools, c) {
                    t.add(&format!("build {}", c.name), b);
                }
                if project.presets.as_ref().and_then(|p| p.test_for(&c.name)).is_some() {
                    if let Some(test) = test_preset(project, tools, c) {
                        t.add(&format!("test {}", c.name), test);
                    }
                }
            }
        }
        None if project.cmake => {
            t.add("configure", configure_plain(project, tools));
            t.add("build", tools.cmake().args(["--build", "build"]));
            build = Some(tools.cmake().args(["--build", "build"]));
            t.add("test", tools.ctest().args(["--test-dir", "build", "--output-on-failure"]));
            t.add("clean", tools.cmake().args(["--build", "build", "--target", "clean"]));
        }
        None => {}
    }
    // Each program alone, as a launch builds it first; not where a
    // preset's task has its name.
    if let Some(b) = build {
        for e in programs {
            let name = crate::launch::build_task(&e.name);
            if !t.tasks.iter().any(|t| t.name == name) {
                t.add(&name, b.clone().args(["--target", &e.name]));
            }
        }
    }
    // A program the build makes isn't one the manifest names, so CMake
    // starts it.
    for e in programs {
        t.add(&format!("run {}", e.name), tools.cmake().args(["-E", "env", &e.program]));
    }
    // vcpkg's toolchain, in a preset, is `$env{VCPKG_ROOT}/...`.
    if let (Some(_), Some(root)) = (&project.vcpkg, tools.vcpkg_root()) {
        t.env.push(("VCPKG_ROOT".into(), root));
    }
    t
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
    fn presets_give_each_its_tasks() {
        let dir = temp("tasks");
        std::fs::write(dir.join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.join("CMakePresets.json"), include_str!("samples/CMakePresets.json")).unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { cmake: Some("/usr/bin/cmake".into()), ..Tools::default() };
        let app = Executable { name: "orbit_app".into(), program: "/home/u/orbit/build/debug/bin/orbit".into(), source: "/home/u/orbit".into() };
        assert_eq!(
            table(&tasks(&project, &tools, None, &[app])),
            rows(&[
                ("configure", "/usr/bin/cmake --preset debug"),
                ("clean", "/usr/bin/cmake --build --preset debug --target clean"),
                ("build", "/usr/bin/cmake --build --preset debug"),
                ("test", "ctest --preset debug"),
                ("configure release", "/usr/bin/cmake --preset release"),
                ("build release", "/usr/bin/cmake --build --preset release"),
                ("build orbit_app", "/usr/bin/cmake --build --preset debug --target orbit_app"),
                ("run orbit_app", "/usr/bin/cmake -E env /home/u/orbit/build/debug/bin/orbit"),
            ]),
            "release has no test preset"
        );
        let t = table(&tasks(&project, &tools, Some("release"), &[]));
        assert_eq!(t[0], ("configure".to_string(), "/usr/bin/cmake --preset release".to_string()));
        assert_eq!(t[3], ("test".to_string(), "ctest --test-dir build/release --output-on-failure".to_string()), "no test preset: its folder");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_preset_that_doesnt_export_commands_is_told_to() {
        let dir = temp("tasks-conan");
        std::fs::create_dir_all(dir.join("build/Release/generators")).unwrap();
        std::fs::write(dir.join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.join("conanfile.txt"), "[requires]\nhello/1.0\n[layout]\ncmake_layout\n").unwrap();
        std::fs::write(dir.join("CMakeUserPresets.json"), include_str!("samples/conan-CMakeUserPresets.json")).unwrap();
        std::fs::write(dir.join("build/Release/generators/CMakePresets.json"), include_str!("samples/conan-CMakePresets.json")).unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { conan: Some("/u/conan".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, None, &[])),
            rows(&[
                ("conan install", "/u/conan install . --build=missing -s build_type=Debug"),
                ("configure", "cmake --preset conan-release -DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
                ("clean", "cmake --build --preset conan-release --target clean"),
                ("build", "cmake --build --preset conan-release"),
                ("test", "ctest --preset conan-release"),
            ])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_presets_its_built_in_build() {
        let dir = temp("tasks-plain");
        std::fs::write(dir.join("CMakeLists.txt"), "").unwrap();
        std::fs::write(dir.join("vcpkg.json"), "{}").unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { ninja: Some("/usr/bin/ninja".into()), vcpkg: Some("/home/u/vcpkg/vcpkg".into()), ..Tools::default() };
        let release = Executable { name: "release".into(), program: "/p/build/release".into(), source: "/p".into() };
        let t = tasks(&project, &tools, None, &[release]);
        assert_eq!(
            table(&t),
            rows(&[
                ("configure", "cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -G Ninja -DCMAKE_TOOLCHAIN_FILE=/home/u/vcpkg/scripts/buildsystems/vcpkg.cmake"),
                ("build", "cmake --build build"),
                ("test", "ctest --test-dir build --output-on-failure"),
                ("clean", "cmake --build build --target clean"),
                ("build release", "cmake --build build --target release"),
                ("run release", "cmake -E env /p/build/release"),
            ])
        );
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("VCPKG_ROOT".to_string(), "/home/u/vcpkg".to_string())]));

        std::fs::remove_file(dir.join("vcpkg.json")).unwrap();
        std::fs::write(dir.join("conanfile.txt"), "[requires]\nhello/1.0\n").unwrap();
        let t = table(&tasks(&Project::read(&dir.to_string_lossy()), &Tools::default(), None, &[]));
        assert_eq!(t[0].1, "conan install . --build=missing -s build_type=Debug --output-folder=build");
        assert!(t[1].1.ends_with("-DCMAKE_TOOLCHAIN_FILE=conan_toolchain.cmake"), "{t:?}");
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(tasks(&Project::default(), &Tools::default(), None, &[]).tasks.is_empty(), "not C or C++");
    }
}
