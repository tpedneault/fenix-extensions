//! The gRPC client's parts that aren't its page: the servers it calls
//! (`protobuf.targets`), where it learns their services -- the server's
//! reflection, or the project's own `.proto` files -- and grpcurl's
//! command for each thing it asks. The extension speaks no HTTP/2 itself:
//! grpcurl does, and the client reads what it prints.

pub mod call;
pub mod describe;
pub mod history;
pub mod skeleton;

use fenix_extension_api::Command;

/// A server to call: `host:port`, and whether it's TLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub address: String,
    pub tls: bool,
}

impl Target {
    pub fn new(address: &str, tls: bool) -> Target {
        Target { address: address.trim().to_string(), tls }
    }

    /// As the client's header and the picker say it.
    pub fn label(&self) -> String {
        format!("{} {}", self.address, if self.tls { "TLS" } else { "plaintext" })
    }
}

/// Whether a TLS column's value is on: `on`, `true`, `yes`, `tls`.
fn on(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "on" | "true" | "yes" | "tls" | "1")
}

/// `protobuf.targets`, a map setting (`[["localhost:50051", "off"],
/// ...]`, its columns the target and TLS); a list of addresses reads
/// too, `tls://` before one that's TLS. The ones that look like
/// `host:port`.
pub fn parse_targets(setting: &str) -> Vec<Target> {
    let v: serde_json::Value = serde_json::from_str(setting.trim()).unwrap_or_default();
    let mut out: Vec<Target> = Vec::new();
    let rows: Vec<(String, bool)> = match &v {
        serde_json::Value::Array(rows) => rows
            .iter()
            .filter_map(|r| match r {
                serde_json::Value::Array(pair) => Some((pair.first()?.as_str()?.to_string(), pair.get(1).and_then(|t| t.as_str()).is_some_and(on))),
                serde_json::Value::String(s) => Some((s.clone(), false)),
                _ => None,
            })
            .collect(),
        // Not JSON: addresses a `,` or a line apart.
        _ => setting.split([',', '\n']).map(|s| (s.to_string(), false)).collect(),
    };
    for (address, tls) in rows {
        let a = address.trim();
        let (a, tls) = match a.strip_prefix("tls://").or_else(|| a.strip_prefix("https://")) {
            Some(rest) => (rest, true),
            None => (a.strip_prefix("http://").unwrap_or(a), tls),
        };
        let a = a.trim_end_matches('/');
        if a.rsplit_once(':').is_some_and(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok()) && !out.iter().any(|t| t.address == a) {
            out.push(Target::new(a, tls));
        }
    }
    out
}

/// The targets as the setting keeps them.
pub fn targets_setting(targets: &[Target]) -> String {
    let rows: Vec<[&str; 2]> = targets.iter().map(|t| [t.address.as_str(), if t.tls { "on" } else { "off" }]).collect();
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
}

/// Where a call's services and messages are described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The server's reflection service.
    Reflection,
    /// A descriptor set `buf build` wrote (`-protoset`): the buf
    /// workspace's modules and their deps.
    Protoset(String),
    /// The `.proto` files, each below the folder it imports from
    /// (`-import-path`, `-proto`).
    Protos(Vec<(String, String)>),
}

impl Source {
    pub fn word(&self) -> &'static str {
        match self {
            Source::Reflection => "reflection",
            Source::Protoset(_) | Source::Protos(_) => "the project's protos",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            Source::Reflection => Vec::new(),
            Source::Protoset(path) => vec!["-protoset".into(), path.clone()],
            Source::Protos(files) => {
                let mut out = Vec::new();
                let mut imports: Vec<&str> = files.iter().map(|(i, _)| i.as_str()).collect();
                imports.dedup();
                for i in imports {
                    out.extend(["-import-path".to_string(), i.to_string()]);
                }
                for (_, f) in files {
                    out.extend(["-proto".to_string(), f.clone()]);
                }
                out
            }
        }
    }
}

/// How grpcurl is run: the program found, the server, how long it may
/// take, and TLS without checking the server's certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grpcurl {
    pub program: String,
    pub target: Option<Target>,
    pub source: Source,
    /// `-insecure`: a TLS server whose certificate doesn't check out.
    pub insecure: bool,
    /// Seconds to connect (`-connect-timeout`) and for the whole call
    /// (`-max-time`, 0 none).
    pub connect_timeout: u32,
    pub max_time: u32,
}

impl Grpcurl {
    fn base(&self, call: bool) -> Command {
        let mut args: Vec<String> = Vec::new();
        // With the project's protos, list and describe need no server.
        let server = call || self.source == Source::Reflection;
        if server {
            match &self.target {
                Some(t) if !t.tls => args.push("-plaintext".into()),
                Some(_) if self.insecure => args.push("-insecure".into()),
                _ => {}
            }
            args.extend(["-connect-timeout".to_string(), self.connect_timeout.max(1).to_string()]);
        }
        args.extend(self.source.args());
        Command::new(&self.program).args(args).cwd(crate::PROJECT)
    }

    fn address(&self, cmd: Command, call: bool) -> Command {
        let server = call || self.source == Source::Reflection;
        match (&self.target, server) {
            (Some(t), true) => cmd.arg(t.address.clone()),
            _ => cmd,
        }
    }

    /// `grpcurl ... list`: the services.
    pub fn list(&self) -> Command {
        self.address(self.base(false), false).arg("list")
    }

    /// `grpcurl ... describe <symbol>`: a service, a message, an enum.
    pub fn describe(&self, symbol: &str) -> Command {
        self.address(self.base(false), false).args(["describe", symbol])
    }

    /// `grpcurl -v ... -H k:v -d <body> <target> <service/method>`: a call,
    /// verbose so its headers, trailers and status come with it.
    pub fn invoke(&self, method: &str, metadata: &[(String, String)], body: &str) -> Command {
        let mut cmd = self.base(true).arg("-v");
        if self.max_time > 0 {
            cmd = cmd.args(["-max-time".to_string(), self.max_time.to_string()]);
        }
        for (k, v) in metadata {
            cmd = cmd.args(["-H".to_string(), format!("{k}: {v}")]);
        }
        cmd = cmd.args(["-d".to_string(), body.to_string()]);
        self.address(cmd, true).arg(method)
    }
}

/// `buf build`'s descriptor set for the client, in the extension's own
/// folder: its imports in it, so grpcurl needs nothing else.
pub fn build_protoset(buf: &str, cwd: &str, out: &str) -> Command {
    Command::new(buf).args(["build", "--as-file-descriptor-set", "-o", out]).cwd(cwd)
}

/// A metadata line typed (`x-team: ops`, `authorization=Bearer x`): its
/// name, lower case as gRPC sends it, and value.
pub fn parse_metadata(line: &str) -> Option<(String, String)> {
    // The first `:` or `=` splits it.
    let at = line.find([':', '='])?;
    let (k, v) = (&line[..at], &line[at + 1..]);
    let k = k.trim().to_ascii_lowercase();
    (!k.is_empty() && !k.contains(' ')).then(|| (k, v.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::line;

    fn grpcurl(source: Source, tls: bool) -> Grpcurl {
        Grpcurl { program: "/t/grpcurl".into(), target: Some(Target::new("localhost:50051", tls)), source, insecure: false, connect_timeout: 5, max_time: 30 }
    }

    #[test]
    fn targets_come_from_the_setting() {
        let t = parse_targets(r#"[["localhost:50051", "off"], ["api.example.com:443", "on"], ["nonsense", "on"], ["localhost:50051", "on"]]"#);
        assert_eq!(t, [Target::new("localhost:50051", false), Target::new("api.example.com:443", true)], "one each, host:port only");
        assert_eq!(parse_targets("localhost:9000, tls://grpc.example.com:443"), [Target::new("localhost:9000", false), Target::new("grpc.example.com:443", true)]);
        assert_eq!(parse_targets(r#"["127.0.0.1:50071"]"#), [Target::new("127.0.0.1:50071", false)]);
        assert!(parse_targets("").is_empty());
        assert_eq!(targets_setting(&t), r#"[["localhost:50051","off"],["api.example.com:443","on"]]"#);
        assert_eq!(parse_targets(&targets_setting(&t)), t);
        assert_eq!(t[1].label(), "api.example.com:443 TLS");
    }

    #[test]
    fn reflection_asks_the_server_and_protos_need_none() {
        assert_eq!(line(&grpcurl(Source::Reflection, false).list()), "/t/grpcurl -plaintext -connect-timeout 5 localhost:50051 list");
        assert_eq!(line(&grpcurl(Source::Reflection, true).describe("orbit.v1.OrbitService")), "/t/grpcurl -connect-timeout 5 localhost:50051 describe orbit.v1.OrbitService");
        let protos = Source::Protos(vec![("proto".into(), "orbit/v1/orbit.proto".into()), ("proto".into(), "orbit/v1/ground.proto".into())]);
        assert_eq!(line(&grpcurl(protos.clone(), false).list()), "/t/grpcurl -import-path proto -proto orbit/v1/orbit.proto -proto orbit/v1/ground.proto list");
        let set = Source::Protoset("${data}/schema.binpb".into());
        assert_eq!(line(&grpcurl(set, false).describe("orbit.v1.Satellite")), "/t/grpcurl -protoset ${data}/schema.binpb describe orbit.v1.Satellite");
        assert_eq!(grpcurl(protos, false).list().cwd.as_deref(), Some("/project"));
    }

    #[test]
    fn a_call_has_its_metadata_body_and_limits() {
        let meta = [("x-team".to_string(), "ops".to_string())];
        let mut g = grpcurl(Source::Reflection, true);
        g.insecure = true;
        let cmd = g.invoke("orbit.v1.OrbitService/GetSatellite", &meta, "{\"id\": \"25544\"}");
        assert_eq!(line(&cmd), "/t/grpcurl -insecure -connect-timeout 5 -v -max-time 30 -H x-team: ops -d {\"id\": \"25544\"} localhost:50051 orbit.v1.OrbitService/GetSatellite");
        assert_eq!(cmd.args[cmd.args.len() - 3], "{\"id\": \"25544\"}", "the body one argument, as it is");
        let protos = Source::Protos(vec![(".".into(), "orbit.proto".into())]);
        let cmd = grpcurl(protos, false).invoke("a.S/M", &[], "{}");
        assert_eq!(line(&cmd), "/t/grpcurl -plaintext -connect-timeout 5 -import-path . -proto orbit.proto -v -max-time 30 -d {} localhost:50051 a.S/M", "a call needs the server whatever describes it");
        assert_eq!(line(&build_protoset("/t/buf", "/project/api", "${data}/schema.binpb")), "/t/buf build --as-file-descriptor-set -o ${data}/schema.binpb");
    }

    #[test]
    fn metadata_lines() {
        assert_eq!(parse_metadata("X-Team: ops"), Some(("x-team".into(), "ops".into())));
        assert_eq!(parse_metadata("authorization=Bearer a:b"), Some(("authorization".into(), "Bearer a:b".into())));
        assert_eq!(parse_metadata("authorization: Bearer a=b"), Some(("authorization".into(), "Bearer a=b".into())), "the first : splits");
        assert_eq!(parse_metadata("no separator"), None);
        assert_eq!(parse_metadata(": x"), None);
    }
}
