//! What the solution is, as its files say: the `.sln` (or `.slnx`) and
//! its projects -- or, without one, the project files found -- each
//! project's SDK, output type, frameworks, assembly, packages and where
//! its build goes; `global.json`'s SDK; the local tools manifest; and a
//! web project's launch profile. `Directory.Build.props` is read for
//! what it sets every project.
//!
//! Paths are relative to the project's folder, with `/` between.

use std::path::Path;

use fenix_lang_kit::serde_json;

/// The project folders not looked in for project files.
const SKIP: &[&str] = &["bin", "obj", "node_modules", "artifacts", "packages", "TestResults"];

/// Project files the SDK builds.
pub const PROJECT_EXTENSIONS: &[&str] = &["csproj", "fsproj", "vbproj"];

/// Packages that make a project a test project.
const TEST_PACKAGES: &[&str] = &["Microsoft.NET.Test.Sdk", "xunit", "xunit.v3", "NUnit", "MSTest", "MSTest.TestFramework", "TUnit"];

/// Packages whose tests are a program of their own (Microsoft.Testing.
/// Platform): run, and debugged, as the project's dll.
const RUNNER_PACKAGES: &[&str] = &["xunit.v3", "TUnit"];

/// A web project's first launch profile that runs the project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    /// Its first `applicationUrl`.
    pub url: Option<String>,
    pub env: Vec<(String, String)>,
}

/// One project of the solution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Project {
    /// Its project file: `src/Orbit.App/Orbit.App.csproj`.
    pub path: String,
    /// The file's name without its extension: what MSBuild calls it.
    pub name: String,
    /// `Microsoft.NET.Sdk`, `Microsoft.NET.Sdk.Web`, `MSTest.Sdk`.
    pub sdk: String,
    pub output_type: Option<String>,
    /// `TargetFramework`, or each of `TargetFrameworks`.
    pub frameworks: Vec<String>,
    pub assembly: String,
    pub test: bool,
    /// A test project that's a program (xunit.v3, TUnit, the MSTest
    /// runner): its tests run, and are debugged, as its dll.
    pub test_program: bool,
    /// `PackageReference`s: id and version, when it says one here.
    pub packages: Vec<(String, Option<String>)>,
    /// Where its build goes, relative to the project folder: its dll's
    /// folder.
    pub out_dir: String,
    pub profile: Option<Profile>,
}

impl Project {
    /// Its folder.
    pub fn dir(&self) -> &str {
        self.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
    }

    pub fn web(&self) -> bool {
        self.sdk.eq_ignore_ascii_case("Microsoft.NET.Sdk.Web")
    }

    pub fn worker(&self) -> bool {
        self.sdk.eq_ignore_ascii_case("Microsoft.NET.Sdk.Worker")
    }

    /// A program: `Exe` (or `WinExe`), which a web or worker project is
    /// unless it says otherwise.
    pub fn runnable(&self) -> bool {
        match self.output_type.as_deref() {
            Some(t) => t.eq_ignore_ascii_case("exe") || t.eq_ignore_ascii_case("winexe"),
            None => self.web() || self.worker() || self.test_program,
        }
    }

    /// Its dll, as its Debug build leaves it.
    pub fn dll(&self) -> String {
        format!("{}/{}.dll", self.out_dir, self.assembly)
    }

    /// The environment it's debugged in, as `dotnet run` gives it: in
    /// Development, its launch profile's variables over that, and the
    /// profile's URL (`ASPNETCORE_URLS`). It runs in its own folder, which
    /// is a hosted app's content root.
    pub fn launch_env(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = ["ASPNETCORE_ENVIRONMENT", "DOTNET_ENVIRONMENT"].iter().map(|k| (k.to_string(), "Development".to_string())).collect();
        let mut set = |k: &str, v: &str| match env.iter_mut().find(|(key, _)| key == k) {
            Some(e) => e.1 = v.to_string(),
            None => env.push((k.to_string(), v.to_string())),
        };
        if let Some(p) = &self.profile {
            for (k, v) in &p.env {
                set(k, v);
            }
            if let Some(url) = &p.url {
                set("ASPNETCORE_URLS", url);
            }
        }
        env
    }

    pub fn has(&self, package: &str) -> bool {
        self.packages.iter().any(|(id, _)| id.eq_ignore_ascii_case(package))
    }
}

/// `global.json`'s `sdk`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlobalJson {
    pub version: Option<String>,
    pub roll_forward: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Solution {
    /// The solution file, when there is one: `Orbit.sln`.
    pub file: Option<String>,
    /// Its projects, in its order; or, without one, those found.
    pub projects: Vec<Project>,
    pub global_json: Option<GlobalJson>,
    /// CSharpier's version in the local tools manifest
    /// (`.config/dotnet-tools.json`).
    pub csharpier: Option<String>,
    /// A local tools manifest, which `dotnet tool restore` installs.
    pub tools_manifest: bool,
}

/// `a` joined to `b`, `..` and `.` taken out: `src/App/../Core` is
/// `src/Core`.
pub fn join(a: &str, b: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in a.split('/').chain(b.split('/')) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// A `.sln`'s projects, in its order: `Project("{type}") = "Name",
/// "src\Name\Name.csproj", "{id}"` lines, solution folders left out.
pub fn parse_sln(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Project(")?;
            let (_, rest) = rest.split_once('=')?;
            let path = rest.split(',').nth(1)?.trim().trim_matches('"').replace('\\', "/");
            let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
            PROJECT_EXTENSIONS.contains(&ext.as_str()).then_some(path)
        })
        .collect()
}

/// A `.slnx`'s projects: each `<Project Path="...">`, in its folders.
pub fn parse_slnx(text: &str) -> Vec<String> {
    let Ok(doc) = roxmltree::Document::parse(text.trim_start_matches('\u{feff}')) else { return Vec::new() };
    doc.descendants().filter(|n| n.has_tag_name("Project")).filter_map(|n| n.attribute("Path")).map(|p| p.replace('\\', "/")).collect()
}

/// What a project file (or `Directory.Build.props`) says: its SDK, its
/// properties (the first of each name, whatever its condition), its
/// package references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Msbuild {
    pub sdk: Option<String>,
    pub properties: Vec<(String, String)>,
    pub packages: Vec<(String, Option<String>)>,
}

impl Msbuild {
    pub fn parse(text: &str) -> Result<Msbuild, String> {
        let doc = roxmltree::Document::parse(text.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())?;
        let root = doc.root_element();
        let mut out = Msbuild { sdk: root.attribute("Sdk").map(|s| s.split('/').next().unwrap_or(s).trim().to_string()), ..Msbuild::default() };
        if out.sdk.is_none() {
            out.sdk = root.children().find(|n| n.has_tag_name("Sdk")).and_then(|n| n.attribute("Name")).map(str::to_string);
        }
        for group in root.children().filter(|n| n.has_tag_name("PropertyGroup")) {
            for p in group.children().filter(|n| n.is_element()) {
                let name = p.tag_name().name().to_string();
                if !out.properties.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                    out.properties.push((name, p.text().unwrap_or("").trim().to_string()));
                }
            }
        }
        for r in root.descendants().filter(|n| n.has_tag_name("PackageReference")) {
            let Some(id) = r.attribute("Include").or_else(|| r.attribute("Update")) else { continue };
            let version = r.attribute("Version").map(str::to_string).or_else(|| r.children().find(|c| c.has_tag_name("Version")).and_then(|c| c.text()).map(|t| t.trim().to_string()));
            out.packages.push((id.to_string(), version));
        }
        Ok(out)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.properties.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }

    fn yes(&self, name: &str) -> Option<bool> {
        self.get(name).map(|v| v.eq_ignore_ascii_case("true"))
    }
}

/// A property's value that's a plain path, or one in the file's own
/// folder: anything else of MSBuild's (`$(Configuration)`) can't be
/// worked out here.
fn plain(value: &str, this_dir: &str) -> Option<String> {
    let v = value.replace('\\', "/").replace("$(MSBuildThisFileDirectory)", &format!("{this_dir}/")).replace("$(MSBuildProjectDirectory)", this_dir);
    (!v.contains("$(")).then_some(v)
}

/// `launchSettings.json`'s first profile that runs the project.
pub fn parse_launch_settings(text: &str) -> Option<Profile> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let profiles = v.get("profiles")?.as_object()?;
    let (name, p) = profiles.iter().find(|(_, p)| p.get("commandName").and_then(|c| c.as_str()) == Some("Project"))?;
    let url = p.get("applicationUrl").and_then(|u| u.as_str()).and_then(|u| u.split(';').map(str::trim).find(|u| u.starts_with("http://")).or_else(|| u.split(';').next())).map(str::to_string);
    let env = p.get("environmentVariables").and_then(|e| e.as_object()).map(|e| e.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default();
    Some(Profile { name: name.clone(), url, env })
}

/// The `Directory.Build.props` that applies to folder `dir` (relative to
/// `root`): the nearest, looking up to the root. Its folder, and it.
fn build_props(root: &Path, dir: &str) -> Option<(String, Msbuild)> {
    let mut d = dir.to_string();
    loop {
        let file = root.join(&d).join("Directory.Build.props");
        if let Ok(text) = std::fs::read_to_string(&file) {
            return Msbuild::parse(&text).ok().map(|m| (d, m));
        }
        if d.is_empty() {
            return None;
        }
        d = d.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default();
    }
}

/// Project `path` (relative to `root`) as its file says.
pub fn read_project(root: &Path, path: &str) -> Option<Project> {
    let text = std::fs::read_to_string(root.join(path)).ok()?;
    let own = Msbuild::parse(&text).ok()?;
    let props = build_props(root, path.rsplit_once('/').map(|(d, _)| d).unwrap_or(""));
    let mut p = project(path, &own, props.as_ref().map(|(d, m)| (d.as_str(), m)));
    let settings = root.join(p.dir()).join("Properties/launchSettings.json");
    p.profile = std::fs::read_to_string(settings).ok().and_then(|t| parse_launch_settings(&t));
    Some(p)
}

/// Project `path` from what its file says, and what the
/// `Directory.Build.props` in folder `props.0` does.
pub fn project(path: &str, own: &Msbuild, props: Option<(&str, &Msbuild)>) -> Project {
    let name = path.rsplit('/').next().unwrap_or(path).rsplit_once('.').map(|(n, _)| n).unwrap_or(path).to_string();
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("").to_string();
    // Its own value, else the props'; each with the folder it's from.
    let get = |key: &str| own.get(key).map(|v| (v.to_string(), dir.clone())).or_else(|| props.and_then(|(d, m)| m.get(key).map(|v| (v.to_string(), d.to_string()))));
    let yes = |key: &str| own.yes(key).or_else(|| props.and_then(|(_, m)| m.yes(key)));
    let frameworks: Vec<String> = match get("TargetFramework") {
        Some((f, _)) => vec![f],
        None => get("TargetFrameworks").map(|(f, _)| f.split(';').map(str::trim).filter(|f| !f.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
    };
    let mut packages = own.packages.clone();
    if let Some((_, m)) = props {
        packages.extend(m.packages.iter().filter(|(id, _)| !own.packages.iter().any(|(o, _)| o.eq_ignore_ascii_case(id))).cloned());
    }
    let has = |names: &[&str]| packages.iter().any(|(id, _)| names.iter().any(|n| n.eq_ignore_ascii_case(id)));
    let sdk = own.sdk.clone().unwrap_or_else(|| "Microsoft.NET.Sdk".into());
    let test = yes("IsTestProject").unwrap_or(false) || sdk.eq_ignore_ascii_case("MSTest.Sdk") || has(TEST_PACKAGES);
    let test_program = test && (has(RUNNER_PACKAGES) || sdk.eq_ignore_ascii_case("MSTest.Sdk") || yes("EnableMSTestRunner") == Some(true) || yes("UseMicrosoftTestingPlatformRunner") == Some(true) || own.get("OutputType").is_some_and(|t| t.eq_ignore_ascii_case("exe")));
    let assembly = get("AssemblyName").and_then(|(a, _)| (!a.contains("$(")).then_some(a)).unwrap_or_else(|| name.clone());
    let tfm = frameworks.first().cloned().unwrap_or_default();
    let rid = get("RuntimeIdentifier").map(|(r, _)| r).filter(|r| !r.contains("$("));

    // Where the build goes: the artifacts layout (`UseArtifactsOutput`),
    // `OutputPath` or `BaseOutputPath` when they're plain paths, else
    // bin/Debug/<framework>/, each with the framework and the runtime
    // folders their properties add.
    let out_dir = if yes("UseArtifactsOutput") == Some(true) {
        let base = get("ArtifactsPath").and_then(|(a, d)| plain(&a, &d).map(|a| join(&d, &a))).unwrap_or_else(|| {
            // Next to the file that asked for it.
            let from = if own.yes("UseArtifactsOutput").is_some() { dir.clone() } else { props.map(|(d, _)| d.to_string()).unwrap_or_default() };
            join(&from, "artifacts")
        });
        let mut pivot = "debug".to_string();
        if frameworks.len() > 1 {
            pivot.push('_');
            pivot.push_str(&tfm);
        }
        if let Some(rid) = &rid {
            pivot.push('_');
            pivot.push_str(rid);
        }
        join(&base, &format!("bin/{name}/{pivot}"))
    } else {
        let mut out = match (get("OutputPath").and_then(|(o, d)| plain(&o, &d).map(|o| (o, d))), get("BaseOutputPath").and_then(|(b, d)| plain(&b, &d).map(|b| (b, d)))) {
            (Some((o, d)), _) => join(&d, &o),
            (None, Some((b, d))) => join(&join(&d, &b), "Debug"),
            (None, None) => join(&dir, "bin/Debug"),
        };
        if yes("AppendTargetFrameworkToOutputPath") != Some(false) && !tfm.is_empty() {
            out = join(&out, &tfm);
        }
        if let (Some(rid), true) = (&rid, yes("AppendRuntimeIdentifierToOutputPath") != Some(false)) {
            out = join(&out, rid);
        }
        out
    };
    Project {
        path: path.to_string(),
        name,
        sdk,
        output_type: own.get("OutputType").or_else(|| props.and_then(|(_, m)| m.get("OutputType"))).map(str::to_string),
        frameworks,
        assembly,
        test,
        test_program,
        packages,
        out_dir,
        profile: None,
    }
}

/// Project files under `root`, three folders down at most, sorted.
fn find_projects(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            if is_dir {
                if depth < 3 && !name.starts_with('.') && !SKIP.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                    walk(root, &rel, depth + 1, out);
                }
            } else if name.rsplit_once('.').is_some_and(|(_, x)| PROJECT_EXTENSIONS.iter().any(|p| p.eq_ignore_ascii_case(x))) {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", 0, &mut out);
    out.sort();
    out
}

/// The tools a local manifest lists: CSharpier's version, if it's one.
pub fn manifest_csharpier(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let tools = v.get("tools")?.as_object()?;
    let (_, t) = tools.iter().find(|(k, _)| k.eq_ignore_ascii_case("csharpier"))?;
    Some(t.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string())
}

pub fn parse_global_json(text: &str) -> Option<GlobalJson> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let sdk = v.get("sdk");
    let field = |k: &str| sdk.and_then(|s| s.get(k)).and_then(|s| s.as_str()).map(str::to_string);
    Some(GlobalJson { version: field("version"), roll_forward: field("rollForward") })
}

impl Solution {
    /// The solution in `root`: `prefer` (a solution file's name) when it's
    /// there, else the first `.sln` or `.slnx` at the top.
    pub fn read(root: &str, prefer: Option<&str>) -> Solution {
        let root_path = Path::new(root);
        let mut slns: Vec<String> = std::fs::read_dir(root_path)
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".sln") || n.ends_with(".slnx")).collect())
            .unwrap_or_default();
        slns.sort();
        let file = prefer.filter(|p| slns.iter().any(|s| s == p)).map(str::to_string).or_else(|| slns.first().cloned());
        let paths = match &file {
            Some(f) => {
                let text = std::fs::read_to_string(root_path.join(f)).unwrap_or_default();
                if f.ends_with(".slnx") {
                    parse_slnx(&text)
                } else {
                    parse_sln(&text)
                }
            }
            None => find_projects(root_path),
        };
        let projects = paths.iter().filter_map(|p| read_project(root_path, &join("", p))).collect();
        let manifest = [".config/dotnet-tools.json", "dotnet-tools.json"].iter().find_map(|m| std::fs::read_to_string(root_path.join(m)).ok());
        Solution {
            file,
            projects,
            global_json: std::fs::read_to_string(root_path.join("global.json")).ok().and_then(|t| parse_global_json(&t)),
            csharpier: manifest.as_deref().and_then(manifest_csharpier),
            tools_manifest: manifest.is_some(),
        }
    }

    /// What `dotnet build`, `test` and `list package` are given: the
    /// solution, else the one project; none runs them in the folder.
    pub fn target(&self) -> Option<String> {
        self.file.clone().or_else(|| (self.projects.len() == 1).then(|| self.projects[0].path.clone()))
    }

    /// The project file `file` (relative) is in: the deepest project
    /// folder holding it.
    pub fn project_of(&self, file: &str) -> Option<&Project> {
        let file = file.replace('\\', "/");
        self.projects.iter().filter(|p| p.path == file || p.dir().is_empty() || file.starts_with(&format!("{}/", p.dir()))).max_by_key(|p| p.dir().len())
    }

    pub fn by_name(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// The newest framework a project targets, as a version: `net9.0` is
    /// `9.0`. What the SDK has to be at least.
    pub fn newest_framework(&self) -> Option<String> {
        self.projects
            .iter()
            .flat_map(|p| &p.frameworks)
            .filter_map(|f| f.strip_prefix("net").and_then(|v| v.split('-').next()).filter(|v| v.contains('.')).and_then(fenix_lang_kit::Version::parse).map(|v| (v.clone(), format!("{}.{}", v.major(), v.number(1)))))
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, s)| s)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fenix-dotnet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn write(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The solution `dotnet new` made: a console app, a library, an
    /// xUnit project, a web API and a worker.
    pub fn orbit(name: &str) -> std::path::PathBuf {
        let dir = temp(name);
        write(&dir, "Orbit.sln", include_str!("samples/Orbit.sln"));
        write(&dir, "src/Orbit.App/Orbit.App.csproj", include_str!("samples/Orbit.App.csproj"));
        write(&dir, "src/Orbit.Core/Orbit.Core.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework>\n  </PropertyGroup>\n</Project>\n");
        write(&dir, "tests/Orbit.Tests/Orbit.Tests.csproj", include_str!("samples/Orbit.Tests.csproj"));
        write(&dir, "src/Orbit.Api/Orbit.Api.csproj", include_str!("samples/Orbit.Api.csproj"));
        write(&dir, "src/Orbit.Api/Properties/launchSettings.json", include_str!("samples/launchSettings.json"));
        write(&dir, "src/Orbit.Worker/Orbit.Worker.csproj", include_str!("samples/Orbit.Worker.csproj"));
        dir
    }

    #[test]
    fn a_solutions_projects_are_read_in_its_order() {
        let dir = orbit("sln");
        let s = Solution::read(&dir.to_string_lossy(), None);
        assert_eq!(s.file.as_deref(), Some("Orbit.sln"));
        let names: Vec<&str> = s.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Orbit.App", "Orbit.Core", "Orbit.Tests", "Orbit.Api", "Orbit.Worker"], "solution folders left out");
        let app = &s.projects[0];
        assert_eq!((app.path.as_str(), app.runnable(), app.test), ("src/Orbit.App/Orbit.App.csproj", true, false));
        assert_eq!(app.dll(), "src/Orbit.App/bin/Debug/net8.0/Orbit.App.dll");
        assert_eq!(app.packages, [("Newtonsoft.Json".to_string(), Some("12.0.1".to_string()))]);
        assert_eq!(app.launch_env().len(), 2, "a console app: Development only");
        assert!(!s.projects[1].runnable(), "a library");
        let tests = &s.projects[2];
        assert!(tests.test && !tests.test_program && !tests.runnable(), "VSTest's: run by dotnet test");
        let api = s.by_name("Orbit.Api").unwrap();
        assert!(api.web() && api.runnable());
        assert_eq!(api.profile.as_ref().map(|p| (p.name.as_str(), p.url.as_deref())), Some(("http", Some("http://localhost:5189"))));
        assert_eq!(api.launch_env().last(), Some(&("ASPNETCORE_URLS".to_string(), "http://localhost:5189".to_string())));
        let worker = s.by_name("Orbit.Worker").unwrap();
        assert!(worker.worker() && worker.runnable());
        assert_eq!(worker.launch_env().len(), 2);
        assert_eq!(s.target().as_deref(), Some("Orbit.sln"));
        assert_eq!(s.project_of("src/Orbit.Api/Controllers/X.cs").map(|p| p.name.as_str()), Some("Orbit.Api"));
        assert_eq!(s.project_of("README.md"), None);
        assert_eq!(s.newest_framework().as_deref(), Some("8.0"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn without_a_solution_the_project_files_are_found() {
        let dir = temp("nosln");
        write(&dir, "App.csproj", "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFrameworks>net8.0;net9.0</TargetFrameworks><AssemblyName>app</AssemblyName></PropertyGroup></Project>");
        write(&dir, "bin/Debug/net8.0/Stale.csproj", "<Project/>");
        write(&dir, ".config/dotnet-tools.json", "{\"version\":1,\"isRoot\":true,\"tools\":{\"csharpier\":{\"version\":\"1.3.0\",\"commands\":[\"csharpier\"]}}}");
        write(&dir, "global.json", "{\n  \"sdk\": {\n    \"version\": \"9.0.100\",\n    \"rollForward\": \"latestFeature\"\n  }\n}\n");
        let s = Solution::read(&dir.to_string_lossy(), None);
        assert_eq!(s.projects.len(), 1, "bin/ isn't looked in");
        assert_eq!(s.projects[0].dll(), "bin/Debug/net8.0/app.dll", "the first framework's, by its assembly name");
        assert_eq!(s.target().as_deref(), Some("App.csproj"));
        assert_eq!(s.csharpier.as_deref(), Some("1.3.0"));
        assert_eq!(s.global_json, Some(GlobalJson { version: Some("9.0.100".into()), roll_forward: Some("latestFeature".into()) }));
        assert_eq!(s.newest_framework().as_deref(), Some("9.0"));
        assert_eq!(s.project_of("Program.cs").map(|p| p.name.as_str()), Some("App"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn where_the_build_goes_follows_the_properties() {
        let m = |text: &str| Msbuild::parse(text).unwrap();
        let p = project("src/A/A.csproj", &m("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><TargetFramework>net9.0</TargetFramework><RuntimeIdentifier>linux-x64</RuntimeIdentifier></PropertyGroup></Project>"), None);
        assert_eq!(p.out_dir, "src/A/bin/Debug/net9.0/linux-x64");
        let p = project("src/A/A.csproj", &m("<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputPath>..\\..\\out</OutputPath><AppendTargetFrameworkToOutputPath>false</AppendTargetFrameworkToOutputPath></PropertyGroup></Project>"), None);
        assert_eq!(p.out_dir, "out");
        let p = project("src/A/A.csproj", &m("<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework><OutputPath>bin\\$(Configuration)</OutputPath></PropertyGroup></Project>"), None);
        assert_eq!(p.out_dir, "src/A/bin/Debug/net8.0", "an MSBuild expression isn't guessed at");
        // Directory.Build.props at the top: the artifacts layout.
        let props = m("<Project><PropertyGroup><UseArtifactsOutput>true</UseArtifactsOutput><TargetFramework>net8.0</TargetFramework></PropertyGroup><ItemGroup><PackageReference Include=\"StyleCop.Analyzers\" Version=\"1.1.118\" /></ItemGroup></Project>");
        let p = project("src/A/A.csproj", &m("<Project Sdk=\"Microsoft.NET.Sdk\"/>"), Some(("", &props)));
        assert_eq!((p.out_dir.as_str(), p.frameworks.clone()), ("artifacts/bin/A/debug", vec!["net8.0".to_string()]));
        assert!(p.has("stylecop.analyzers"));
        let p = project("src/A/A.csproj", &m("<Project><PropertyGroup><TargetFrameworks>net8.0;net9.0</TargetFrameworks></PropertyGroup></Project>"), Some(("", &m("<Project><PropertyGroup><UseArtifactsOutput>true</UseArtifactsOutput><ArtifactsPath>$(MSBuildThisFileDirectory).build</ArtifactsPath></PropertyGroup></Project>"))));
        assert_eq!(p.out_dir, ".build/bin/A/debug_net8.0");
        // Tests that are a program of their own.
        let p = project("T/T.csproj", &m("<Project Sdk=\"Microsoft.NET.Sdk\"><ItemGroup><PackageReference Include=\"xunit.v3\"><Version>1.0.0</Version></PackageReference></ItemGroup></Project>"), None);
        assert!(p.test && p.test_program && p.runnable());
        assert_eq!(p.packages, [("xunit.v3".to_string(), Some("1.0.0".to_string()))]);
        let p = project("T/T.csproj", &m("<Project Sdk=\"MSTest.Sdk/3.6.4\"/>"), None);
        assert!(p.test && p.test_program && p.sdk == "MSTest.Sdk");
    }

    #[test]
    fn paths_and_solution_files_read_as_msbuild_writes_them() {
        assert_eq!(join("src/App", "../Core/Core.csproj"), "src/Core/Core.csproj");
        assert_eq!(join("", "./a/b"), "a/b");
        let slnx = "<Solution>\n  <Folder Name=\"/src/\">\n    <Project Path=\"src\\App\\App.csproj\" />\n  </Folder>\n  <Project Path=\"Lib/Lib.fsproj\" />\n</Solution>\n";
        assert_eq!(parse_slnx(slnx), ["src/App/App.csproj", "Lib/Lib.fsproj"]);
        assert_eq!(parse_sln("Project(\"{F2A71F9B-5D33-465A-A702-920D77279786}\") = \"Lib\", \"Lib\\Lib.fsproj\", \"{1}\"\nProject(\"{2150E333-8FDC-42A3-9474-1A3956D46DE8}\") = \"src\", \"src\", \"{2}\""), ["Lib/Lib.fsproj"]);
        assert!(Msbuild::parse("<Project").is_err());
        assert_eq!(parse_launch_settings("{\"profiles\":{\"IIS\":{\"commandName\":\"IISExpress\"}}}"), None);
        let https = parse_launch_settings("{\"profiles\":{\"https\":{\"commandName\":\"Project\",\"applicationUrl\":\"https://localhost:7078;http://localhost:5189\",\"environmentVariables\":{\"ASPNETCORE_ENVIRONMENT\":\"Staging\"}}}}").unwrap();
        assert_eq!((https.url.as_deref(), https.env.clone()), (Some("http://localhost:5189"), vec![("ASPNETCORE_ENVIRONMENT".to_string(), "Staging".to_string())]), "http, which needs no certificate");
    }
}
