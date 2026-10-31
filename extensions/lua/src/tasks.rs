//! The tasks (`SPC p t`): run `main.lua`, or the game with LÖVE; test
//! with busted; `luarocks make` and `luarocks test` for each rockspec;
//! lint and format the whole project with the tools it uses. The
//! project's `.env` goes to each; a task of the same name in
//! `.fenix/tools.json` wins.

use fenix_extension_api::Command;
use fenix_lang_kit::Tasks;

use crate::lint::LintWith;
use crate::project::Project;
use crate::tools::Tools;

/// The tasks for `project` in folder `root`, with what's on this machine;
/// `stylua` and `linter` are what the settings make of it.
pub fn tasks(project: &Project, tools: &Tools, root: &str, stylua: bool, linter: Option<LintWith>) -> Tasks {
    let mut t = Tasks::default();
    if !project.lua {
        return t;
    }
    if project.love {
        t.add("love", Command::new(tools.love.as_deref().unwrap_or("love")).arg("."));
    } else if project.main {
        t.add("run", tools.interpreter().arg("main.lua"));
    }
    if project.busted {
        t.add("test", Command::new(tools.busted.as_deref().unwrap_or("busted")));
    }
    let one = project.rockspecs.len() == 1;
    for rockspec in &project.rockspecs {
        let name = rockspec.trim_end_matches(".rockspec");
        let named = |verb: &str| if one { format!("luarocks {verb}") } else { format!("luarocks {verb} {name}") };
        t.add(&named("make"), tools.luarocks().args(["make", rockspec]));
        t.add(&named("test"), tools.luarocks().args(["test", rockspec]));
    }
    match (linter, &tools.selene, &tools.luacheck) {
        (Some(LintWith::Selene), Some(selene), _) => t.add("lint", Command::new(selene).arg(".")),
        (Some(LintWith::Luacheck), _, Some(luacheck)) => t.add("lint", Command::new(luacheck).arg(".")),
        _ => {}
    }
    if let (true, Some(stylua)) = (stylua, &tools.stylua) {
        t.add("format", Command::new(stylua).arg("."));
    }
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
    fn a_rock_with_its_tools_gets_them_all() {
        let project = Project { lua: true, main: true, busted: true, rockspecs: vec!["orbit-1.0-1.rockspec".into(), "orbit-scm-1.rockspec".into()], ..Project::default() };
        let tools = Tools { lua: Some("/usr/bin/lua5.4".into()), busted: Some("/h/.luarocks/bin/busted".into()), selene: Some("/t/selene".into()), stylua: Some("/t/stylua".into()), ..Tools::default() };
        assert_eq!(
            table(&tasks(&project, &tools, "/nowhere", true, Some(LintWith::Selene))),
            rows(&[
                ("run", "/usr/bin/lua5.4 main.lua"),
                ("test", "/h/.luarocks/bin/busted"),
                ("luarocks make orbit-1.0-1", "luarocks make orbit-1.0-1.rockspec"),
                ("luarocks test orbit-1.0-1", "luarocks test orbit-1.0-1.rockspec"),
                ("luarocks make orbit-scm-1", "luarocks make orbit-scm-1.rockspec"),
                ("luarocks test orbit-scm-1", "luarocks test orbit-scm-1.rockspec"),
                ("lint", "/t/selene ."),
                ("format", "/t/stylua ."),
            ])
        );
        let one = Project { rockspecs: vec!["orbit-1.0-1.rockspec".into()], ..project };
        let t = tasks(&one, &Tools { luacheck: Some("/h/luacheck".into()), ..Tools::default() }, "/nowhere", false, Some(LintWith::Luacheck));
        assert_eq!(table(&t)[2..], rows(&[("luarocks make", "luarocks make orbit-1.0-1.rockspec"), ("luarocks test", "luarocks test orbit-1.0-1.rockspec"), ("lint", "/h/luacheck .")]), "one rockspec: plain names; no StyLua, no format");
    }

    #[test]
    fn a_love_game_is_run_with_love_and_its_env() {
        let dir = temp("tasks");
        std::fs::write(dir.join(".env"), "LOVE_DEBUG=1\n").unwrap();
        let project = Project { lua: true, main: true, love: true, ..Project::default() };
        let t = tasks(&project, &Tools::default(), &dir.to_string_lossy(), true, Some(LintWith::Selene));
        assert_eq!(table(&t), rows(&[("love", "love .")]), "nothing installed: no lint, no format");
        assert!(t.resolved().iter().all(|t| t.cmd.env == [("LOVE_DEBUG".to_string(), "1".to_string())]));
        assert!(tasks(&Project::default(), &Tools::default(), "/nowhere", true, None).tasks.is_empty(), "no Lua, no tasks");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
