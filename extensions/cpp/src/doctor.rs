//! The project doctor's C and C++ checks: a compiler, CMake held to what
//! the project asks (`cmake_minimum_required`, its presets' version),
//! Ninja, clangd, clang-format, clang-tidy, the debugger, vcpkg or Conan
//! (and Conan's profile) when the project uses them, and the build's
//! `compile_commands.json`. What PyPI has is installed by a fix row
//! (`uv tool install`, else pipx, else pip); a missing database is made
//! by configuring.

use fenix_extension_api::{Check, Command, Health, Section};
use fenix_lang_kit::{Fix, Output, Program};

use crate::project::Project;
use crate::tools::{hints, program_name, vcpkg_hints, Debugger, Tools};

/// The fix rows' commands, in the manifest.
pub const INSTALL_CMAKE: &str = "cpp.install-cmake";
pub const INSTALL_NINJA: &str = "cpp.install-ninja";
pub const INSTALL_CLANGD: &str = "cpp.install-clangd";
pub const INSTALL_CLANG_FORMAT: &str = "cpp.install-clang-format";
pub const INSTALL_CLANG_TIDY: &str = "cpp.install-clang-tidy";
pub const INSTALL_CONAN: &str = "cpp.install-conan";
pub const CONAN_PROFILE: &str = "cpp.conan-profile";
pub const INSTALL_COMPILER: &str = "cpp.install-compiler";
pub const INSTALL_LLVM: &str = "cpp.install-llvm";
/// The pack's own: configures, then checks again.
pub const CONFIGURE: &str = "cpp.configure";

/// GDB speaks DAP from 14 on.
pub const GDB_DAP: &str = "14";

/// `conan profile path default`: there's a default profile.
pub fn profile(out: &Output) -> (Health, String) {
    if out.ok() {
        (Health::Ok, format!("the default profile is {}", out.stdout.trim()))
    } else {
        (Health::Bad, format!("no default profile: {}", out.why()))
    }
}

fn program(name: &str, label: &str) -> Program {
    hints().into_iter().fold(Program::new(name, label), Program::hint)
}

fn fix(program: Program, label: &str, command: &str, cmd: Option<Command>) -> Program {
    match cmd {
        Some(cmd) => program.fix(label, command, Fix::Task(cmd)),
        None => program,
    }
}

/// The checks for `project`, with what's on this machine; `preset` is
/// `cpp.preset`.
pub fn programs(project: &Project, tools: &Tools, preset: Option<&str>, debugger: Debugger) -> Vec<Program> {
    let mut out = Vec::new();
    if !project.is_cpp() {
        return out;
    }
    // The compiler found first; MSVC's `cl` says its version only when
    // it's given nothing, so it's only found.
    let compiler = tools.compiler.as_ref().map(|(name, _)| name.as_str()).unwrap_or("c++");
    let c = program(compiler, "Compiler");
    let c = if compiler == "cl" { c.version_args(Vec::<String>::new()) } else { c };
    let label = if tools.os == "macos" { "Install Apple's command line tools" } else { "Install Visual Studio's build tools" };
    out.push(fix(c, label, INSTALL_COMPILER, tools.install_compiler()));

    if project.cmake || project.presets.is_some() {
        let cmake = program("cmake", "CMake");
        let cmake = match project.cmake_needs() {
            Some(v) => cmake.at_least(&v),
            None => cmake,
        };
        out.push(fix(cmake, "Install CMake", INSTALL_CMAKE, tools.install_tool("cmake")));
        let ninja = program("ninja", "Ninja");
        // Wanted by the preset, or on Windows without one: configuring
        // without Ninja there makes a Visual Studio solution, which writes
        // no compile_commands.json for clangd and clang-tidy.
        let ninja = if project.wants_ninja(preset) || visual_studio_by_default(project, tools, preset) { ninja } else { ninja.optional() };
        out.push(fix(ninja, "Install Ninja", INSTALL_NINJA, tools.install_tool("ninja")));
    }

    let clangd = program("clangd", "clangd").optional();
    out.push(fix(clangd, "Install clangd", INSTALL_CLANGD, tools.install_tool("clangd")));
    let format = program("clang-format", "clang-format").optional();
    out.push(fix(format, "Install clang-format", INSTALL_CLANG_FORMAT, tools.install_tool("clang-format")));
    let tidy = program("clang-tidy", "clang-tidy").optional();
    out.push(fix(tidy, "Install clang-tidy", INSTALL_CLANG_TIDY, tools.install_tool("clang-tidy")));

    // The debugger: lldb-dap is only found (it has no --version before
    // LLVM 19); GDB needs 14 for DAP.
    let found = tools.adapter(debugger).map(|(d, _)| d);
    out.push(match found.unwrap_or(debugger) {
        Debugger::LldbDap => {
            let name = tools.lldb_dap.as_deref().map(program_name).unwrap_or_else(|| "lldb-dap".into());
            let p = program(&name, "Debugger").version_args(Vec::<String>::new()).optional();
            fix(p, "Install LLVM (lldb-dap)", INSTALL_LLVM, tools.install_llvm())
        }
        Debugger::Gdb => program("gdb", "Debugger").at_least(GDB_DAP).optional(),
    });

    if project.vcpkg.is_some() {
        out.push(vcpkg_hints().into_iter().fold(Program::new("vcpkg", "vcpkg"), Program::hint).version_args(["version"]));
    }
    if project.conan.is_some() {
        out.push(fix(program("conan", "Conan"), "Install Conan", INSTALL_CONAN, tools.install_tool("conan")));
        let p = program("conan", "Conan profile").section(Section::Project).version_args(["profile", "path", "default"]).judge(profile);
        out.push(fix(p, "Make one for this machine (conan profile detect)", CONAN_PROFILE, Some(tools.conan().args(["profile", "detect"]))));
    }
    out
}

/// Whether configuring makes a Visual Studio solution: on Windows, with no
/// preset to say otherwise, CMake's default generator there.
fn visual_studio_by_default(project: &Project, tools: &Tools, preset: Option<&str>) -> bool {
    tools.os == "windows" && project.preset(preset).is_none()
}

/// The check that the build wrote `compile_commands.json`, which clangd
/// and clang-tidy read: where it is, else what to do.
pub fn database(project: &Project, tools: &Tools, root: &str, preset: Option<&str>) -> Option<Check> {
    if !project.is_cpp() {
        return None;
    }
    let label = "compile_commands.json";
    if let Some(dir) = project.database(root, preset) {
        return Some(Check::new(Section::Project, label, Health::Ok, format!("{dir}/compile_commands.json")));
    }
    let Some(dir) = project.database_dir(preset) else {
        return Some(Check::new(Section::Project, label, Health::Warn, "there's none, and no CMake to write one: clangd guesses each file's flags"));
    };
    if project.preset(preset).is_some_and(|c| c.visual_studio()) {
        return Some(Check::new(Section::Project, label, Health::Warn, "Visual Studio's generator doesn't write one: use Ninja in the preset"));
    }
    if tools.ninja.is_none() && visual_studio_by_default(project, tools, preset) {
        return Some(Check::new(Section::Project, label, Health::Warn, "without Ninja, CMake makes a Visual Studio solution here, which writes none: install Ninja, then configure").fix("Install Ninja", INSTALL_NINJA));
    }
    let how = match project.preset(preset) {
        Some(c) => format!("cmake --preset {}", c.name),
        None => "cmake -B build".into(),
    };
    Some(Check::new(Section::Project, label, Health::Warn, format!("none in {dir} yet: configure the project ({how})")).fix("Configure", CONFIGURE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::tests::temp;

    fn out(code: i32, stdout: &str, stderr: &str) -> Output {
        Output { code: Some(code), stdout: stdout.into(), stderr: stderr.into() }
    }

    fn rows(programs: &[Program]) -> Vec<(String, String, bool, Option<String>)> {
        programs.iter().map(|p| (p.label.clone(), p.name.clone(), p.required, p.fix.as_ref().map(|f| f.1.clone()))).collect()
    }

    fn cmake_project(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        std::fs::write(dir.join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.29)\n").unwrap();
        std::fs::write(dir.join("CMakePresets.json"), include_str!("samples/CMakePresets.json")).unwrap();
        std::fs::write(dir.join("conanfile.txt"), "[requires]\nhello/1.0\n").unwrap();
        dir
    }

    #[test]
    fn a_cmake_project_checks_its_toolchain() {
        let dir = cmake_project("doctor");
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { compiler: Some(("g++".into(), "/usr/bin/g++".into())), gdb: Some("/usr/bin/gdb".into()), uv: Some("/u/uv".into()), ..Tools::default() };
        let p = programs(&project, &tools, None, Debugger::LldbDap);
        assert_eq!(
            rows(&p),
            [
                ("Compiler".into(), "g++".into(), true, None),
                ("CMake".into(), "cmake".into(), true, Some(INSTALL_CMAKE.into())),
                ("Ninja".into(), "ninja".into(), true, Some(INSTALL_NINJA.into())),
                ("clangd".into(), "clangd".into(), false, Some(INSTALL_CLANGD.into())),
                ("clang-format".into(), "clang-format".into(), false, Some(INSTALL_CLANG_FORMAT.into())),
                ("clang-tidy".into(), "clang-tidy".into(), false, Some(INSTALL_CLANG_TIDY.into())),
                ("Debugger".into(), "gdb".into(), false, None),
                ("Conan".into(), "conan".into(), true, Some(INSTALL_CONAN.into())),
                ("Conan profile".into(), "conan".into(), true, Some(CONAN_PROFILE.into())),
            ],
            "the preset's generator is Ninja; there's no lldb-dap, so GDB's checked"
        );
        // CMake held to cmake_minimum_required's 3.29, more than the presets' 3.25.
        let c = p[1].evaluate(Some("/usr/bin/cmake"), Some(&out(0, "cmake version 3.28.3\n\nCMake suite maintained and supported by Kitware (kitware.com/cmake).\n", "")));
        assert_eq!((c.health, c.detail.as_str()), (Health::Bad, "3.28.3, but the project needs >=3.29 (/usr/bin/cmake)"));
        assert_eq!(c.fix, Some(("Install CMake".into(), INSTALL_CMAKE.into())));
        match &p[1].fix {
            Some((_, _, Fix::Task(cmd))) => assert_eq!(crate::tests::line(cmd), "/u/uv tool install cmake"),
            other => panic!("{other:?}"),
        }
        let gdb = p[6].evaluate(Some("/usr/bin/gdb"), Some(&out(0, "GNU gdb (Ubuntu 15.1-1ubuntu1~24.04.1) 15.1\nCopyright (C) 2024 Free Software Foundation, Inc.\n", "")));
        assert_eq!(gdb.health, Health::Ok);
        let old = p[6].evaluate(Some("/usr/bin/gdb"), Some(&out(0, "GNU gdb (GDB) Red Hat Enterprise Linux 12.1-2.el9\n", "")));
        assert_eq!(old.health, Health::Warn, "too old for DAP");
        let gxx = p[0].evaluate(Some("/usr/bin/g++"), Some(&out(0, "g++ (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0\nCopyright (C) 2023 Free Software Foundation, Inc.\n", "")));
        assert_eq!(gxx.detail, "13.3.0-6ubuntu2~24.04.1 (/usr/bin/g++)", "lang-kit reads the first version: Ubuntu's");
        let ninja = p[2].evaluate(Some("/usr/bin/ninja"), Some(&out(0, "1.11.1\n", "")));
        assert_eq!(ninja.health, Health::Ok);
        // Conan's profile, read as it says it.
        assert_eq!(p[8].version_args, ["profile", "path", "default"]);
        let none = p[8].evaluate(Some("/u/conan"), Some(&out(1, "", "ERROR: Profile not found: default\n")));
        assert_eq!((none.health, none.detail.as_str()), (Health::Bad, "no default profile: ERROR: Profile not found: default"));
        assert_eq!(p[8].evaluate(Some("/u/conan"), Some(&out(0, "/home/u/.conan2/profiles/default\n", ""))).health, Health::Ok);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lldb_dap_and_msvc_are_only_found() {
        let dir = temp("doctor-msvc");
        std::fs::write(dir.join("main.cpp"), "").unwrap();
        std::fs::write(dir.join("vcpkg.json"), "{}").unwrap();
        let project = Project::read(&dir.to_string_lossy());
        let tools = Tools { compiler: Some(("cl".into(), "C:\\VS\\cl.exe".into())), lldb_dap: Some("/usr/lib/llvm-18/bin/lldb-dap".into()), ..Tools::default() };
        let p = programs(&project, &tools, None, Debugger::Gdb);
        let labels: Vec<&str> = p.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["Compiler", "clangd", "clang-format", "clang-tidy", "Debugger", "vcpkg"], "no CMake, so no CMake or Ninja");
        assert!(p[0].version_args.is_empty() && p[4].version_args.is_empty());
        assert_eq!(p[4].name, "lldb-dap", "GDB was asked for, but lldb-dap is what's there");
        assert!(p.iter().all(|p| p.fix.is_none()), "nothing to install with");
        assert_eq!(p[5].version_args, ["version"]);
        assert!(p[5].hints.contains(&"$VCPKG_ROOT".to_string()));
        assert!(programs(&Project::default(), &tools, None, Debugger::Gdb).is_empty(), "not C or C++: no checks");
        let mac = Tools { os: "macos".into(), xcode_select: Some("/usr/bin/xcode-select".into()), brew: Some("/opt/homebrew/bin/brew".into()), ..Tools::default() };
        let p = programs(&project, &mac, None, Debugger::LldbDap);
        assert_eq!(p[0].fix.as_ref().map(|f| (f.0.as_str(), f.1.as_str())), Some(("Install Apple's command line tools", INSTALL_COMPILER)));
        assert_eq!(p[4].fix.as_ref().map(|f| f.1.as_str()), Some(INSTALL_LLVM));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn compile_commands_json_is_looked_for_where_the_preset_builds() {
        let dir = cmake_project("doctor-db");
        let root = dir.to_string_lossy().into_owned();
        let project = Project::read(&root);
        let none = Tools::default();
        let c = database(&project, &none, &root, None).unwrap();
        assert_eq!((c.health, c.detail.as_str()), (Health::Warn, "none in build/debug yet: configure the project (cmake --preset debug)"));
        assert_eq!(c.fix.as_ref().map(|f| f.1.as_str()), Some(CONFIGURE));
        std::fs::create_dir_all(dir.join("build/debug")).unwrap();
        std::fs::write(dir.join("build/debug/compile_commands.json"), "[]").unwrap();
        let c = database(&project, &none, &root, None).unwrap();
        assert_eq!((c.health, c.detail.as_str(), c.fix.is_none()), (Health::Ok, "build/debug/compile_commands.json", true));
        std::fs::remove_dir_all(&dir).unwrap();

        let vs = temp("doctor-vs");
        std::fs::write(vs.join("CMakePresets.json"), r#"{"version": 3, "configurePresets": [{"name": "vs", "generator": "Visual Studio 17 2022", "binaryDir": "out"}]}"#).unwrap();
        let root = vs.to_string_lossy().into_owned();
        assert!(database(&Project::read(&root), &none, &root, None).unwrap().detail.contains("Visual Studio"));
        std::fs::remove_dir_all(&vs).unwrap();
        let src = temp("doctor-src");
        std::fs::write(src.join("a.c"), "").unwrap();
        let root = src.to_string_lossy().into_owned();
        assert!(database(&Project::read(&root), &none, &root, None).unwrap().detail.contains("guesses"));
        std::fs::remove_dir_all(&src).unwrap();
    }

    /// Windows, CMake and no preset: without Ninja, configuring makes a
    /// Visual Studio solution, which writes no compile_commands.json. So
    /// Ninja is wanted there, and the database's check says to install it.
    #[test]
    fn on_windows_without_a_preset_ninja_is_wanted() {
        let dir = temp("doctor-win");
        std::fs::write(dir.join("CMakeLists.txt"), "cmake_minimum_required(VERSION 3.20)\nproject(hello CXX)\nadd_executable(hello main.cpp)\n").unwrap();
        std::fs::write(dir.join("main.cpp"), "int main() {}\n").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let project = Project::read(&root);
        let windows = Tools { os: "windows".into(), ..Tools::default() };
        let ninja = programs(&project, &windows, None, Debugger::LldbDap).into_iter().find(|p| p.name == "ninja").unwrap();
        assert!(ninja.required, "wanted on Windows");
        let linux = programs(&project, &Tools::default(), None, Debugger::LldbDap).into_iter().find(|p| p.name == "ninja").unwrap();
        assert!(!linux.required, "Make writes one elsewhere");
        let c = database(&project, &windows, &root, None).unwrap();
        assert!(c.detail.contains("install Ninja"), "{}", c.detail);
        assert_eq!(c.fix.as_ref().map(|f| f.1.as_str()), Some(INSTALL_NINJA));
        let with = Tools { ninja: Some("C:/tools/ninja.exe".into()), ..windows };
        assert_eq!(database(&project, &with, &root, None).unwrap().fix.as_ref().map(|f| f.1.as_str()), Some(CONFIGURE));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
