//! What the engines report, read into one shape: containers, Compose
//! stacks, images, volumes, networks, live stats and disk use. Docker
//! and Podman answer in JSON (one object a line, or an array); wslc in
//! columns, read by their headings. Every reader takes what it can and
//! skips what it can't: an engine's version that adds or renames a field
//! loses that field, not the list.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `created`, `paused`, `restarting`.
    pub state: String,
    /// The engine's own words: `Up 3 hours (healthy)`, `Exited (1) 2 minutes ago`.
    pub status: String,
    pub ports: String,
    /// The Compose stack and service it belongs to, from its labels.
    pub project: Option<String>,
    pub service: Option<String>,
}

impl Container {
    pub fn running(&self) -> bool {
        self.state == "running"
    }

    /// `healthy`, `unhealthy` or `starting`, when it has a healthcheck.
    pub fn health(&self) -> Option<&'static str> {
        ["unhealthy", "healthy", "health: starting"].into_iter().find(|h| self.status.contains(h)).map(|h| if h == "health: starting" { "starting" } else { h })
    }

    /// The exit code of a stopped one, from its status.
    pub fn exit_code(&self) -> Option<i32> {
        let rest = self.status.strip_prefix("Exited (")?;
        rest.split(')').next()?.trim().parse().ok()
    }

    /// The host ports it publishes (`8080` from `0.0.0.0:8080->80/tcp`).
    pub fn host_ports(&self) -> Vec<u16> {
        let mut out: Vec<u16> = self
            .ports
            .split(',')
            .filter_map(|p| p.split("->").next())
            .filter(|p| p.contains(':'))
            .filter_map(|p| p.rsplit(':').next()?.trim().parse().ok())
            .collect();
        out.dedup();
        out
    }
}

/// A field of `v` as text: a string, a number, or the first of an array.
fn text(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(a) => a.first().and_then(|x| x.as_str()).map(str::to_string),
        _ => None,
    }
}

fn first_text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| text(v, k))
}

/// A container's labels, from Docker's `k=v,k=v` or Podman's object.
fn label(v: &Value, key: &str) -> Option<String> {
    match v.get("Labels")? {
        Value::Object(map) => map.get(key).and_then(|x| x.as_str()).map(str::to_string),
        Value::String(s) => s.split(',').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v.to_string())),
        _ => None,
    }
}

/// Podman writes ports as objects; Docker as text.
fn ports(v: &Value) -> String {
    match v.get("Ports") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|p| {
                let host = p.get("host_port")?.as_u64()?;
                let inner = p.get("container_port")?.as_u64()?;
                let proto = p.get("protocol").and_then(|x| x.as_str()).unwrap_or("tcp");
                let ip = p.get("host_ip").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).unwrap_or("0.0.0.0");
                Some(format!("{ip}:{host}->{inner}/{proto}"))
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

pub fn container(v: &Value) -> Option<Container> {
    let id = first_text(v, &["ID", "Id"])?;
    let name = text(v, "Names")?.trim_start_matches('/').to_string();
    let state = first_text(v, &["State"]).unwrap_or_default().to_lowercase();
    Some(Container {
        id,
        name,
        image: text(v, "Image").unwrap_or_default(),
        state,
        status: text(v, "Status").unwrap_or_default(),
        ports: ports(v),
        project: label(v, "com.docker.compose.project").or_else(|| label(v, "io.podman.compose.project")),
        service: label(v, "com.docker.compose.service"),
    })
}

/// The objects in an engine's JSON answer: one a line, or one array.
pub fn objects(out: &str) -> Vec<Value> {
    let trimmed = out.trim_start();
    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<Value>>(trimmed).unwrap_or_default();
    }
    out.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).filter(|v| v.is_object()).collect()
}

pub fn containers(out: &str) -> Vec<Container> {
    objects(out).iter().filter_map(container).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    pub size: u64,
    /// `3 weeks ago`.
    pub created: String,
}

impl Image {
    /// Untagged: what a rebuild left behind.
    pub fn dangling(&self) -> bool {
        self.repository == "<none>" || self.repository.is_empty()
    }

    pub fn reference(&self) -> String {
        format!("{}:{}", self.repository, self.tag)
    }
}

pub fn images(out: &str) -> Vec<Image> {
    objects(out)
        .iter()
        .filter_map(|v| {
            // Podman: `Names` ["docker.io/library/nginx:1.27"], Size a number.
            let (repository, tag) = match (text(v, "Repository"), text(v, "Tag")) {
                (Some(r), Some(t)) => (r, t),
                _ => match text(v, "Names") {
                    Some(n) => match n.rsplit_once(':') {
                        Some((r, t)) if !t.contains('/') => (r.to_string(), t.to_string()),
                        _ => (n, "latest".into()),
                    },
                    None => ("<none>".into(), "<none>".into()),
                },
            };
            let size = match v.get("Size") {
                Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
                Some(Value::String(s)) => bytes(s),
                _ => 0,
            };
            Some(Image { id: first_text(v, &["ID", "Id"])?, repository, tag, size, created: first_text(v, &["CreatedSince", "CreatedAt"]).unwrap_or_default() })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
}

pub fn volumes(out: &str) -> Vec<Volume> {
    objects(out)
        .iter()
        .filter_map(|v| Some(Volume { name: text(v, "Name")?, driver: text(v, "Driver").unwrap_or_default(), mountpoint: text(v, "Mountpoint").unwrap_or_default() }))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
}

impl Network {
    /// One the engine makes itself, which can't be removed.
    pub fn built_in(&self) -> bool {
        matches!(self.name.as_str(), "bridge" | "host" | "none" | "podman")
    }
}

pub fn networks(out: &str) -> Vec<Network> {
    objects(out)
        .iter()
        .filter_map(|v| Some(Network { id: first_text(v, &["ID", "Id"]).unwrap_or_default(), name: text(v, "Name")?, driver: text(v, "Driver").unwrap_or_default() }))
        .collect()
}

/// A Compose stack, from `compose ls`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stack {
    pub name: String,
    /// `running(3)`, `exited(1)`.
    pub status: String,
    pub files: String,
}

pub fn stacks(out: &str) -> Vec<Stack> {
    objects(out)
        .iter()
        .filter_map(|v| Some(Stack { name: text(v, "Name")?, status: text(v, "Status").unwrap_or_default(), files: text(v, "ConfigFiles").unwrap_or_default() }))
        .collect()
}

/// One container's load, from `stats`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Load {
    pub cpu: f32,
    pub mem: u64,
    /// 0 when it has no limit.
    pub mem_limit: u64,
    pub net_in: u64,
    pub net_out: u64,
    pub disk_read: u64,
    pub disk_write: u64,
    pub pids: u32,
}

/// `(id or name, load)` from one `stats --format json` line.
pub fn load(v: &Value) -> Option<(String, Load)> {
    let id = first_text(v, &["ID", "Container", "Name", "id"])?;
    let pair = |key: &str| -> (u64, u64) {
        let s = text(v, key).unwrap_or_default();
        let mut it = s.split('/').map(|x| bytes(x.trim()));
        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    };
    let (mem, mem_limit) = pair("MemUsage");
    let (net_in, net_out) = pair("NetIO");
    let (disk_read, disk_write) = pair("BlockIO");
    Some((
        id,
        Load {
            cpu: percent(&first_text(v, &["CPUPerc", "CPU"]).unwrap_or_default()),
            mem,
            mem_limit,
            net_in,
            net_out,
            disk_read,
            disk_write,
            pids: text(v, "PIDs").and_then(|p| p.parse().ok()).unwrap_or(0),
        },
    ))
}

/// `"12.5%"` as 12.5.
pub fn percent(s: &str) -> f32 {
    s.trim().trim_end_matches('%').trim().parse().unwrap_or(0.0)
}

/// `"12.5MiB"`, `"1.2kB"`, `"3GB"`, `"0B"` as bytes.
pub fn bytes(s: &str) -> u64 {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (n, unit) = s.split_at(split);
    let n: f64 = n.parse().unwrap_or(0.0);
    let mult: f64 = match unit.trim() {
        "" | "B" => 1.0,
        "kB" | "KB" | "K" => 1e3,
        "MB" | "M" => 1e6,
        "GB" | "G" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (n * mult) as u64
}

/// Bytes, said briefly: `612 MiB`, `1.2 GiB`, `31 kB`.
pub fn size(n: u64) -> String {
    const K: f64 = 1024.0;
    let n = n as f64;
    if n < K {
        format!("{n:.0} B")
    } else if n < K * K {
        format!("{:.0} KiB", n / K)
    } else if n < K * K * K {
        format!("{:.0} MiB", n / (K * K))
    } else {
        format!("{:.1} GiB", n / (K * K * K))
    }
}

/// One kind's line of `system df`.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskUse {
    /// `Images`, `Containers`, `Local Volumes`, `Build Cache`.
    pub kind: String,
    pub count: String,
    pub active: String,
    pub size: u64,
    pub reclaimable: u64,
}

pub fn disk_use(out: &str) -> Vec<DiskUse> {
    objects(out)
        .iter()
        .filter_map(|v| {
            Some(DiskUse {
                kind: text(v, "Type")?,
                count: text(v, "TotalCount").unwrap_or_default(),
                active: text(v, "Active").unwrap_or_default(),
                size: bytes(&text(v, "Size").unwrap_or_default()),
                reclaimable: bytes(text(v, "Reclaimable").unwrap_or_default().split(' ').next().unwrap_or("")),
            })
        })
        .collect()
}

/// One layer of `history`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub created_by: String,
    pub size: u64,
}

pub fn layers(out: &str) -> Vec<Layer> {
    objects(out)
        .iter()
        .filter_map(|v| {
            let by = text(v, "CreatedBy")?;
            // `/bin/sh -c #(nop) CMD ["nginx"]` reads as `CMD ["nginx"]`.
            let by = by.trim_start_matches("/bin/sh -c ").trim_start_matches("#(nop) ").trim().to_string();
            let size = match v.get("Size") {
                Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
                _ => bytes(&text(v, "Size").unwrap_or_default()),
            };
            Some(Layer { created_by: by, size })
        })
        .collect()
}

/// What `inspect` says of a container, for its page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Details {
    pub command: String,
    pub started: String,
    pub restarts: u64,
    pub env: Vec<(String, String)>,
    /// `(source, destination, kind, read-write)`.
    pub mounts: Vec<(String, String, String, bool)>,
    /// `(network, address, aliases)`.
    pub networks: Vec<(String, String, String)>,
    pub health_log: Vec<String>,
}

pub fn details(out: &str) -> Option<Details> {
    let v = objects(out).into_iter().next()?;
    let config = v.get("Config");
    let list = |x: Option<&Value>| x.and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let words = |x: Option<&Value>| list(x).iter().filter_map(|w| w.as_str().map(str::to_string)).collect::<Vec<_>>();
    let mut command = words(config.and_then(|c| c.get("Entrypoint")));
    command.extend(words(config.and_then(|c| c.get("Cmd"))));
    let env = words(config.and_then(|c| c.get("Env"))).into_iter().filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect();
    let mounts = list(v.get("Mounts"))
        .iter()
        .map(|m| {
            let s = |k: &str| m.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
            let source = if s("Type") == "volume" { s("Name") } else { s("Source") };
            (source, s("Destination"), s("Type"), m.get("RW").and_then(|x| x.as_bool()).unwrap_or(true))
        })
        .collect();
    let networks = v
        .pointer("/NetworkSettings/Networks")
        .and_then(|n| n.as_object())
        .map(|map| {
            map.iter()
                .map(|(name, n)| {
                    let aliases = n.get("Aliases").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                    (name.clone(), n.get("IPAddress").and_then(|x| x.as_str()).unwrap_or_default().to_string(), aliases)
                })
                .collect()
        })
        .unwrap_or_default();
    let health_log = list(v.pointer("/State/Health/Log"))
        .iter()
        .rev()
        .take(5)
        .map(|h| {
            let code = h.get("ExitCode").and_then(|x| x.as_i64()).unwrap_or(0);
            let out = h.get("Output").and_then(|x| x.as_str()).unwrap_or_default().trim().lines().next().unwrap_or_default().to_string();
            format!("{} {out}", if code == 0 { "ok" } else { "failed" })
        })
        .collect();
    Some(Details {
        command: command.join(" "),
        started: v.pointer("/State/StartedAt").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
        restarts: v.get("RestartCount").and_then(|x| x.as_u64()).unwrap_or(0),
        env,
        mounts,
        networks,
        health_log,
    })
}

/// Whether an environment variable's value is better not shown.
pub fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["TOKEN", "PASSWORD", "PASSWD", "SECRET", "KEY", "CREDENTIAL"].iter().any(|w| n.contains(w))
}

/// A table of columns, as wslc writes it: a heading line, then rows,
/// each field under its heading. `(heading, value)` pairs a row.
pub fn table(out: &str) -> Vec<Vec<(String, String)>> {
    let mut lines = out.lines().filter(|l| !l.trim().is_empty());
    let Some(head) = lines.next() else { return Vec::new() };
    // Where each heading starts: after two spaces or more.
    let mut starts = Vec::new();
    let chars: Vec<char> = head.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != ' ' && (i == 0 || (i >= 2 && chars[i - 1] == ' ' && chars[i - 2] == ' ')) {
            starts.push(i);
        }
        i += 1;
    }
    let field = |line: &[char], from: usize, to: Option<usize>| -> String {
        let end = to.unwrap_or(line.len()).min(line.len());
        if from >= end {
            return String::new();
        }
        line[from..end].iter().collect::<String>().trim().to_string()
    };
    let headings: Vec<String> = starts.iter().enumerate().map(|(n, &s)| field(&chars, s, starts.get(n + 1).copied())).collect();
    lines
        .map(|line| {
            let row: Vec<char> = line.chars().collect();
            headings.iter().enumerate().map(|(n, h)| (h.to_ascii_uppercase(), field(&row, starts[n], starts.get(n + 1).copied()))).collect()
        })
        .collect()
}

/// wslc's `container list --all`, read as containers.
pub fn wslc_containers(out: &str) -> Vec<Container> {
    table(out)
        .into_iter()
        .filter_map(|row| {
            let get = |names: &[&str]| row.iter().find(|(h, _)| names.contains(&h.as_str())).map(|(_, v)| v.clone()).unwrap_or_default();
            let status = get(&["STATUS"]);
            let state = if status.starts_with("Up") || status.eq_ignore_ascii_case("running") { "running" } else if status.starts_with("Exited") || status.eq_ignore_ascii_case("exited") { "exited" } else { "created" };
            let id = get(&["CONTAINER ID", "ID"]);
            (!id.is_empty()).then(|| Container { id, name: get(&["NAMES", "NAME"]), image: get(&["IMAGE"]), state: state.into(), status, ports: get(&["PORTS"]), project: None, service: None })
        })
        .collect()
}

/// wslc's `image list`, read as images.
pub fn wslc_images(out: &str) -> Vec<Image> {
    table(out)
        .into_iter()
        .filter_map(|row| {
            let get = |names: &[&str]| row.iter().find(|(h, _)| names.contains(&h.as_str())).map(|(_, v)| v.clone()).unwrap_or_default();
            let id = get(&["IMAGE ID", "ID"]);
            (!id.is_empty()).then(|| Image { id, repository: get(&["REPOSITORY", "NAME"]), tag: get(&["TAG"]), size: bytes(&get(&["SIZE"]).replace(' ', "")), created: get(&["CREATED"]) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_ps_is_read_with_its_stack_health_and_ports() {
        let out = r#"{"Command":"\"/docker-entrypoint.…\"","CreatedAt":"2026-10-24 12:00:00 -0400 EDT","ID":"8a1f3c2d9e","Image":"nginx:1.27","Labels":"com.docker.compose.project=shop,com.docker.compose.service=web,com.docker.compose.version=2.31","Names":"shop-web-1","Ports":"0.0.0.0:8080->80/tcp, [::]:8080->80/tcp","State":"running","Status":"Up 2 hours (healthy)"}
{"ID":"77aa","Image":"shop-api:dev","Labels":"","Names":"migrate","Ports":"","State":"exited","Status":"Exited (1) 12 minutes ago"}"#;
        let cs = containers(out);
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].project.as_deref(), cs[0].service.as_deref()), (Some("shop"), Some("web")));
        assert_eq!(cs[0].health(), Some("healthy"));
        assert_eq!(cs[0].host_ports(), [8080]);
        assert!(cs[0].running());
        assert_eq!((cs[1].exit_code(), cs[1].project.as_deref()), (Some(1), None));
    }

    #[test]
    fn podman_ps_is_read_from_its_array_with_names_and_ports_as_lists() {
        let out = r#"[{"Id":"c0ffee","Names":["db"],"Image":"docker.io/library/postgres:16","State":"running","Status":"Up 3 minutes","Labels":{"io.podman.compose.project":"shop"},"Ports":[{"host_ip":"","container_port":5432,"host_port":5433,"protocol":"tcp"}]}]"#;
        let cs = containers(out);
        assert_eq!((cs[0].id.as_str(), cs[0].name.as_str()), ("c0ffee", "db"));
        assert_eq!(cs[0].ports, "0.0.0.0:5433->5432/tcp");
        assert_eq!(cs[0].host_ports(), [5433]);
        assert_eq!(cs[0].project.as_deref(), Some("shop"));
    }

    #[test]
    fn images_volumes_networks_and_stacks_are_read() {
        let imgs = images("{\"ID\":\"3f9c\",\"Repository\":\"shop-api\",\"Tag\":\"dev\",\"Size\":\"412MB\",\"CreatedSince\":\"2 hours ago\"}\n{\"ID\":\"99\",\"Repository\":\"<none>\",\"Tag\":\"<none>\",\"Size\":\"398MB\",\"CreatedSince\":\"2 hours ago\"}");
        assert_eq!((imgs[0].reference(), imgs[0].size, imgs[1].dangling()), ("shop-api:dev".to_string(), 412_000_000, true));
        let podman = images(r#"[{"Id":"ab","Names":["docker.io/library/nginx:1.27"],"Size":188000000,"CreatedAt":"2026-09-01"}]"#);
        assert_eq!((podman[0].repository.as_str(), podman[0].tag.as_str(), podman[0].size), ("docker.io/library/nginx", "1.27", 188_000_000));
        let vols = volumes("{\"Driver\":\"local\",\"Mountpoint\":\"/var/lib/docker/volumes/shop_pg/_data\",\"Name\":\"shop_pg\"}");
        assert_eq!(vols[0].name, "shop_pg");
        let nets = networks("{\"Driver\":\"bridge\",\"ID\":\"1\",\"Name\":\"bridge\"}\n{\"Driver\":\"bridge\",\"ID\":\"2\",\"Name\":\"shop_default\"}");
        assert!(nets[0].built_in() && !nets[1].built_in());
        let st = stacks(r#"[{"Name":"shop","Status":"running(3)","ConfigFiles":"C:\\code\\shop\\compose.yaml"}]"#);
        assert_eq!((st[0].name.as_str(), st[0].status.as_str()), ("shop", "running(3)"));
    }

    #[test]
    fn stats_and_sizes_are_read() {
        let v: Value = serde_json::from_str(r#"{"BlockIO":"0B / 120kB","CPUPerc":"8.10%","Container":"shop-api-1","ID":"8a1f","MemPerc":"59.77%","MemUsage":"612MiB / 1GiB","Name":"shop-api-1","NetIO":"2.1MB / 900kB","PIDs":"23"}"#).unwrap();
        let (id, l) = load(&v).unwrap();
        assert_eq!(id, "8a1f");
        assert!((l.cpu - 8.1).abs() < 0.01);
        assert_eq!((l.mem, l.mem_limit, l.net_in, l.disk_write, l.pids), (612 * 1024 * 1024, 1024 * 1024 * 1024, 2_100_000, 120_000, 23));
        assert_eq!(size(612 * 1024 * 1024), "612 MiB");
        assert_eq!(size(1_288_490_189), "1.2 GiB");
        assert_eq!(bytes("0B"), 0);
    }

    #[test]
    fn disk_use_layers_and_details_are_read() {
        let df = disk_use("{\"Active\":\"9\",\"Reclaimable\":\"1.6GB (32%)\",\"Size\":\"4.9GB\",\"TotalCount\":\"14\",\"Type\":\"Images\"}");
        assert_eq!((df[0].kind.as_str(), df[0].size, df[0].reclaimable), ("Images", 4_900_000_000, 1_600_000_000));
        let ls = layers("{\"CreatedBy\":\"/bin/sh -c #(nop)  CMD [\\\"node\\\"]\",\"Size\":\"0B\"}\n{\"CreatedBy\":\"RUN npm ci\",\"Size\":\"261MB\"}");
        assert_eq!((ls[0].created_by.as_str(), ls[1].size), ("CMD [\"node\"]", 261_000_000));
        let d = details(r#"[{"Config":{"Entrypoint":["docker-entrypoint.sh"],"Cmd":["node","dist/server.js"],"Env":["NODE_ENV=development","STRIPE_KEY=sk_test_1"]},"RestartCount":2,"State":{"StartedAt":"2026-10-24T12:00:00Z","Health":{"Log":[{"ExitCode":0,"Output":"ok\n"}]}},"Mounts":[{"Type":"bind","Source":"C:\\code\\shop\\api\\src","Destination":"/app/src","RW":true},{"Type":"volume","Name":"shop_node_modules","Destination":"/app/node_modules","RW":true}],"NetworkSettings":{"Networks":{"shop_default":{"IPAddress":"172.18.0.3","Aliases":["api"]}}}}]"#).unwrap();
        assert_eq!(d.command, "docker-entrypoint.sh node dist/server.js");
        assert_eq!(d.restarts, 2);
        assert_eq!(d.mounts[1], ("shop_node_modules".to_string(), "/app/node_modules".to_string(), "volume".to_string(), true));
        assert_eq!(d.networks[0], ("shop_default".to_string(), "172.18.0.3".to_string(), "api".to_string()));
        assert_eq!(d.health_log, ["ok ok"]);
        assert!(secret_name("STRIPE_KEY") && secret_name("db_password") && !secret_name("NODE_ENV"));
    }

    /// wslc writes columns; they're read by their headings.
    #[test]
    fn wslc_tables_are_read_by_their_headings() {
        let out = "CONTAINER ID   IMAGE          COMMAND                  CREATED        STATUS          PORTS                  NAMES\n\
                   4d2c9f1a0b7e   nginx:latest   \"/docker-entrypoint.…\"   2 hours ago    Up 2 hours      0.0.0.0:8080->80/tcp   web\n\
                   9b1e0c3d2a11   ubuntu:24.04   \"bash\"                   5 days ago     Exited (0) 4d                          shell\n";
        let cs = wslc_containers(out);
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].name.as_str(), cs[0].image.as_str(), cs[0].state.as_str()), ("web", "nginx:latest", "running"));
        assert_eq!(cs[0].host_ports(), [8080]);
        assert_eq!((cs[1].state.as_str(), cs[1].exit_code()), ("exited", Some(0)));
        let imgs = wslc_images("REPOSITORY   TAG       IMAGE ID       CREATED       SIZE\nnginx        latest    a1b2c3d4e5f6   3 weeks ago   188MB\n");
        assert_eq!((imgs[0].reference(), imgs[0].size), ("nginx:latest".to_string(), 188_000_000));
    }
}
