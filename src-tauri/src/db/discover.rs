//! Find the database servers running on a host.
//!
//! Two places they hide, and they need different treatment:
//!
//! - **Natively installed** — listening on the host's own network stack, found with
//!   whichever of five probes the host has (see [`DISCOVER_CMD`]); a tunnel to
//!   `127.0.0.1:<port>` reaches them even when the port is firewalled off from the
//!   internet (which it should be).
//! - **Inside Docker** — a container may publish a port to the host, or may only be
//!   reachable on the bridge network. `docker ps` gives the published mapping;
//!   `docker inspect` gives the container IP for the ones that publish nothing. Because
//!   the tunnel's destination is resolved by the SSH server, `172.17.0.x:3306` works
//!   from here just as well as a published port — no `docker exec`, no published port
//!   required, and no changes to how the user runs their containers.
//!
//! Everything is parsed into typed records rather than fed to a model as text, so the UI
//! can list candidates and the user just picks one.
//!
//! # Portability
//!
//! Everything here runs through POSIX `sh` on the remote host, so it works on Linux, the
//! BSDs, macOS, Solaris and AIX. Nothing is assumed to exist: each probe is guarded and
//! the parsers accept every output shape the probes produce, including the BSD `addr.port`
//! address form. Container attribution via `/proc/<pid>/cgroup` is Linux-only by nature
//! and simply finds nothing elsewhere, which costs nothing — FreeBSD jails and Solaris
//! zones are **not** attributed, so a database inside one is reported as a host install.
//! That is correct as long as its client binary is reachable from the host; running the
//! client inside a jail (`jexec`) is not implemented.

use serde::{Deserialize, Serialize};

use crate::ssh::SessionManager;

/// Which server we're talking to. Decides the client binary, the output format, and the
/// identifier quoting — see [`super::query`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbEngine {
    MySql,
    Postgres,
    Redis,
}

impl DbEngine {
    pub fn label(self) -> &'static str {
        match self {
            DbEngine::MySql => "MySQL / MariaDB",
            DbEngine::Postgres => "PostgreSQL",
            DbEngine::Redis => "Redis / Valkey",
        }
    }

    /// Whether this engine speaks SQL. Redis does not, so the SQL-shaped helpers
    /// (identifier quoting, `information_schema` queries) don't apply to it.
    pub fn is_sql(self) -> bool {
        !matches!(self, DbEngine::Redis)
    }
}

/// A database product we can recognise, whether or not we can browse it yet.
///
/// Kept separate from [`DbEngine`]: this is "what did we find", while `DbEngine` is "what
/// can the client actually drive". Recognising a product without pretending to support it
/// is the point — a Redis instance should appear in the list with an honest label rather
/// than being silently dropped, which is what happened to Postgres before this existed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbProduct {
    MySql,
    Postgres,
    MongoDb,
    Redis,
    MsSql,
    ClickHouse,
    Cassandra,
    CouchDb,
    Elasticsearch,
}

impl DbProduct {
    pub fn label(self) -> &'static str {
        match self {
            DbProduct::MySql => "MySQL / MariaDB",
            DbProduct::Postgres => "PostgreSQL",
            DbProduct::MongoDb => "MongoDB",
            DbProduct::Redis => "Redis / Valkey",
            DbProduct::MsSql => "SQL Server",
            DbProduct::ClickHouse => "ClickHouse",
            DbProduct::Cassandra => "Cassandra",
            DbProduct::CouchDb => "CouchDB",
            DbProduct::Elasticsearch => "Elasticsearch",
        }
    }

    /// The engine that can browse this, if any. `None` means "found and listed, but the
    /// client can't open it yet" — the UI says so rather than offering a dead sign-in.
    pub fn engine(self) -> Option<DbEngine> {
        match self {
            DbProduct::MySql => Some(DbEngine::MySql),
            DbProduct::Postgres => Some(DbEngine::Postgres),
            DbProduct::Redis => Some(DbEngine::Redis),
            _ => None,
        }
    }
}

/// Ports that usually mean a database, and which product each implies.
///
/// Port alone is a hint, not proof — a container's image name is the better signal and is
/// preferred when available. Neighbouring ports are included because running a second
/// instance one port up is common.
const DB_PORTS: &[(u16, DbProduct)] = &[
    (3306, DbProduct::MySql),
    (3307, DbProduct::MySql),
    (3308, DbProduct::MySql),
    (33060, DbProduct::MySql),
    (5432, DbProduct::Postgres),
    (5433, DbProduct::Postgres),
    (27017, DbProduct::MongoDb),
    (27018, DbProduct::MongoDb),
    (6379, DbProduct::Redis),
    (6380, DbProduct::Redis),
    (1433, DbProduct::MsSql),
    (8123, DbProduct::ClickHouse),
    (9000, DbProduct::ClickHouse),
    (9042, DbProduct::Cassandra),
    (5984, DbProduct::CouchDb),
    (9200, DbProduct::Elasticsearch),
];

/// Image-name fragments, most specific first. Order matters: `timescale/timescaledb`
/// must be seen as Postgres before any looser match.
const IMAGE_HINTS: &[(&str, DbProduct)] = &[
    ("mariadb", DbProduct::MySql),
    ("percona", DbProduct::MySql),
    ("mysql", DbProduct::MySql),
    ("pgvector", DbProduct::Postgres),
    ("timescale", DbProduct::Postgres),
    ("citus", DbProduct::Postgres),
    ("postgis", DbProduct::Postgres),
    ("postgres", DbProduct::Postgres),
    ("mongo", DbProduct::MongoDb),
    ("valkey", DbProduct::Redis),
    ("redis", DbProduct::Redis),
    ("mssql", DbProduct::MsSql),
    ("sqlserver", DbProduct::MsSql),
    ("clickhouse", DbProduct::ClickHouse),
    ("cassandra", DbProduct::Cassandra),
    ("scylla", DbProduct::Cassandra),
    ("couchdb", DbProduct::CouchDb),
    ("elasticsearch", DbProduct::Elasticsearch),
    ("opensearch", DbProduct::Elasticsearch),
];

/// Where a discovered database lives, and how to reach it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DbEndpoint {
    /// Stable-ish identity for the UI, e.g. `native:3306` or `docker:mariadb-main`.
    pub id: String,
    /// What to show the user.
    pub label: String,
    /// How it was found.
    pub kind: DbKind,
    /// Host to forward to, as the SSH server resolves it (loopback or a container IP).
    pub host: String,
    pub port: u16,
    /// Container name, when it came from Docker.
    pub container: Option<String>,
    /// Container image, when known — the most reliable engine hint.
    pub image: Option<String>,
    /// What was found.
    pub product: DbProduct,
    /// How to browse it, or `None` when the client can't open this product yet.
    pub engine: Option<DbEngine>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DbKind {
    Native,
    Docker,
}

/// One command, so discovery costs a single SSH round trip rather than four.
///
/// Every part is guarded with `|| true` / `2>/dev/null` so a host without `docker`, or
/// without `ss`, still returns usable output for the parts that do exist.
///
/// The listener probe tries five tools in order and takes the first that succeeds, because
/// `ss` is Linux-only and the flags `netstat -ltnp` uses are Linux-only too. On FreeBSD
/// both fail, the section comes back empty, and nothing is ever found — which is exactly
/// what was reported. The chain covers, in order of how much they tell us:
///
/// | Tool | Where | Gives a PID |
/// |---|---|---|
/// | `ss -ltnp` | Linux (iproute2) | yes |
/// | `netstat -ltnp` | Linux (net-tools) | yes |
/// | `sockstat -46lP tcp` | FreeBSD, NetBSD | yes |
/// | `lsof -nP -iTCP -sTCP:LISTEN` | macOS, and anywhere lsof is installed | yes |
/// | `netstat -an` | every Unix, including OpenBSD, Solaris, AIX | no |
///
/// The last is the floor: no process attribution, but it finds the port everywhere.
const DISCOVER_CMD: &str = "\
echo '@@OS'; (uname -s 2>/dev/null) || true; \
echo '@@LISTEN'; (ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null \
 || sockstat -46lP tcp 2>/dev/null || lsof -nP -iTCP -sTCP:LISTEN 2>/dev/null \
 || netstat -an 2>/dev/null) || true; \
echo '@@DOCKER'; (docker ps --no-trunc --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Ports}}' 2>/dev/null) || true; \
echo '@@DOCKERIP'; (docker ps -q 2>/dev/null | xargs -r docker inspect \
 -f '{{.Name}}\t{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' 2>/dev/null) || true; \
echo '@@PIDCG'; (ss -ltnpH 2>/dev/null | sed -n 's/.*pid=\\([0-9]*\\).*/\\1/p' | sort -u \
 | while read p; do printf '%s\t' \"$p\"; tr '\\n' ' ' < /proc/$p/cgroup 2>/dev/null; echo; done) || true";

/// Discover database endpoints reachable on `vps_id`.
pub async fn discover(
    sessions: &SessionManager,
    vps_id: &str,
) -> Result<Vec<DbEndpoint>, String> {
    let out = sessions.run_command(vps_id, DISCOVER_CMD).await?;
    Ok(parse(&out.stdout))
}

/// Which part of the combined output a line belongs to.
#[derive(Clone, Copy)]
enum Section {
    Os,
    Listen,
    Docker,
    DockerIp,
    PidCgroup,
}

/// Split the combined output into its sections and parse each.
///
/// Sections are collected into a fixed array indexed by the marker rather than by
/// juggling `&mut` references to three separate locals — same result, but nothing for a
/// reader (or the borrow checker) to puzzle over.
fn parse(stdout: &str) -> Vec<DbEndpoint> {
    let mut sections: [String; 5] = Default::default();
    // Output from a build that predates the @@OS marker still starts with listeners.
    let mut current = Section::Listen;

    for line in stdout.lines() {
        match line.trim() {
            "@@OS" => {
                current = Section::Os;
                continue;
            }
            "@@LISTEN" => {
                current = Section::Listen;
                continue;
            }
            "@@DOCKER" => {
                current = Section::Docker;
                continue;
            }
            "@@DOCKERIP" => {
                current = Section::DockerIp;
                continue;
            }
            "@@PIDCG" => {
                current = Section::PidCgroup;
                continue;
            }
            _ => {}
        }
        let slot = &mut sections[current as usize];
        slot.push_str(line);
        slot.push('\n');
    }

    let [_os, listen, docker, docker_ip, pid_cgroup] = sections;
    let ips = parse_container_ips(&docker_ip);
    let pid_to_container = parse_pid_cgroups(&pid_cgroup);
    let mut found: Vec<DbEndpoint> = Vec::new();

    // Docker first: a container that publishes 3306 also shows up as a host listener,
    // and the container record is the more informative of the two.
    let mut published: Vec<u16> = Vec::new();
    for c in parse_docker(&docker) {
        // Image first — `postgres:16-alpine` is unambiguous, whereas a host port can be
        // remapped to anything. Fall back to the port when the image is unfamiliar.
        let product = product_for_image(&c.image).or_else(|| {
            c.published
                .iter()
                .chain(c.container_port.iter())
                .find_map(|p| product_for_port(*p))
        });
        let Some(product) = product else {
            continue;
        };
        // Prefer a published host port (no bridge routing needed); fall back to the
        // container's own address on the Docker network. Take the first published port
        // regardless of whether it looks like a database port — the image already told us
        // what this is, and a remapped host port (say 15432->5432) is normal.
        let (host, port) = match c.published.first().copied() {
            Some(p) => {
                published.push(p);
                ("127.0.0.1".to_string(), p)
            }
            None => match ips.iter().find(|(name, _)| *name == c.name) {
                Some((_, ip)) => (ip.clone(), c.container_port.unwrap_or(default_port(product))),
                // No published port and no address we can see — not reachable.
                None => continue,
            },
        };
        found.push(DbEndpoint {
            id: format!("docker:{}", c.name),
            label: format!("{} ({})", c.name, c.image),
            kind: DbKind::Docker,
            host,
            port,
            container: Some(c.name),
            image: Some(c.image),
            product,
            engine: product.engine(),
        });
    }

    // Then host listeners that no container already accounted for.
    let containers = parse_docker(&docker);
    for l in parse_listeners(&listen) {
        if published.contains(&l.port) || found.iter().any(|e| e.port == l.port) {
            continue;
        }
        let Some(product) = product_for_port(l.port) else {
            continue;
        };

        // A listener on the host is not necessarily a host *install*. A container run
        // with `network_mode: host` puts its process straight onto the host's network
        // stack, so it shows up here with no `docker ps` port mapping at all — and the
        // client binary lives inside the container, not on the host. Attributing the
        // listening PID to its container via /proc/<pid>/cgroup is what lets the query
        // layer reach it with `docker exec`; without this it tried to run `mysql` on a
        // host that has no `mysql` installed, and simply failed.
        let owner = l
            .pid
            .and_then(|pid| pid_to_container.get(&pid))
            .and_then(|cid| {
                containers
                    .iter()
                    .find(|c| c.id.starts_with(cid.as_str()) || cid.starts_with(&c.id))
            });

        match owner {
            Some(c) => found.push(DbEndpoint {
                id: format!("docker:{}", c.name),
                label: format!("{} ({})", c.name, c.image),
                kind: DbKind::Docker,
                // Host-networked, so the container's own loopback IS the host's.
                host: "127.0.0.1".to_string(),
                port: l.port,
                container: Some(c.name.clone()),
                image: Some(c.image.clone()),
                product,
                engine: product.engine(),
            }),
            None => found.push(DbEndpoint {
                id: format!("native:{}", l.port),
                label: format!("{} on the host (port {})", product.label(), l.port),
                kind: DbKind::Native,
                // Loopback unless it is bound to one specific address that is not
                // loopback — a FreeBSD jail's IP being the case that matters. Forwarding
                // such a listener to 127.0.0.1 reaches nothing.
                host: l.addr.clone().unwrap_or_else(|| "127.0.0.1".to_string()),
                port: l.port,
                container: None,
                image: None,
                product,
                engine: product.engine(),
            }),
        }
    }

    found
}

/// Map a listening PID to the container id that owns it, from `/proc/<pid>/cgroup`.
///
/// Two layouts in the wild — cgroup v2 `…/docker-<id>.scope` and v1 `…/docker/<id>` —
/// so the id is found by scanning for a long hex run rather than by matching a path shape.
fn parse_pid_cgroups(text: &str) -> std::collections::HashMap<u32, String> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        let mut cols = line.split('\t');
        let (Some(pid), Some(cgroup)) = (cols.next(), cols.next()) else {
            continue;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            continue;
        };
        if let Some(id) = longest_hex_run(cgroup) {
            out.insert(pid, id);
        }
    }
    out
}

/// The longest run of hex characters in `s`, if it is long enough to be a container id.
fn longest_hex_run(s: &str) -> Option<String> {
    let mut best = "";
    let mut start = None;
    let bytes = s.as_bytes();
    for i in 0..=bytes.len() {
        let is_hex = i < bytes.len() && (bytes[i] as char).is_ascii_hexdigit();
        match (is_hex, start) {
            (true, None) => start = Some(i),
            (false, Some(st)) => {
                if i - st > best.len() {
                    best = &s[st..i];
                }
                start = None;
            }
            _ => {}
        }
    }
    // Docker ids are 64 hex chars; 12 is the conventional short form. Anything shorter is
    // a cgroup path component, not an id.
    (best.len() >= 12).then(|| best.to_string())
}

fn product_for_port(port: u16) -> Option<DbProduct> {
    DB_PORTS.iter().find(|(p, _)| *p == port).map(|(_, e)| *e)
}

/// Guess the product from a container image, e.g. `postgres:16-alpine`.
///
/// Covers forks and distributions, not just base images: pgvector, timescale, citus and
/// postgis are Postgres; percona is MySQL; valkey is Redis; scylla speaks Cassandra.
fn product_for_image(image: &str) -> Option<DbProduct> {
    let i = image.to_ascii_lowercase();
    IMAGE_HINTS
        .iter()
        .find(|(fragment, _)| i.contains(fragment))
        .map(|(_, p)| *p)
}

/// Default listening port, for a container that publishes nothing recognisable.
fn default_port(product: DbProduct) -> u16 {
    DB_PORTS
        .iter()
        .find(|(_, p)| *p == product)
        .map(|(port, _)| *port)
        .unwrap_or(0)
}

/// A listening socket that might be a database, and the process behind it.
struct Listener {
    port: u16,
    /// From `users:(("mariadbd",pid=1133344,fd=29))`, when the tool reported it.
    pid: Option<u32>,
    /// The address it is bound to, when that is a specific one rather than a wildcard.
    ///
    /// Loopback is the right destination for almost everything, but not for a database
    /// bound only to one address — a FreeBSD jail's IP being the common case. Forwarding
    /// to 127.0.0.1 then connects to nothing.
    addr: Option<String>,
}

/// Split an address token into host and port, accepting both conventions.
///
/// Linux tools write `127.0.0.1:3306`; the BSDs and Solaris write `127.0.0.1.3306`, and
/// `netstat` on macOS and FreeBSD does the same. Only the colon form was understood, so
/// every BSD listener was invisible even when the probe itself had worked.
fn split_addr_port(token: &str) -> Option<(&str, u16)> {
    // Colon first: an IPv6 address contains colons AND dots, and `[::1]:3306` or
    // `::1.3306` must not be cut at a dot that belongs to the address.
    if let Some((host, tail)) = token.rsplit_once(':') {
        if let Ok(port) = tail.parse::<u16>() {
            return Some((host, port));
        }
    }
    if let Some((host, tail)) = token.rsplit_once('.') {
        if let Ok(port) = tail.parse::<u16>() {
            return Some((host, port));
        }
    }
    None
}

/// A wildcard or loopback bind, i.e. one where forwarding to loopback is correct.
fn is_wildcard_or_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    matches!(h, "" | "*" | "0.0.0.0" | "::" | "::1" | "127.0.0.1") || h.starts_with("127.")
}

/// The PID owning a listening socket, whichever tool reported it.
///
/// Each tool puts it somewhere different, so this identifies the format from the shape of
/// the line rather than assuming a column that only holds for `ss`.
fn pid_from_line(line: &str) -> Option<u32> {
    // ss / netstat -p: `users:(("mariadbd",pid=1133344,fd=29))`.
    if let Some(rest) = line.split("pid=").nth(1) {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(pid) = digits.parse::<u32>() {
            return Some(pid);
        }
    }
    let cols: Vec<&str> = line.split_whitespace().collect();
    // lsof: COMMAND PID USER FD TYPE ... NAME (LISTEN)
    if line.contains("(LISTEN)") {
        return cols.get(1).and_then(|c| c.parse().ok());
    }
    // sockstat: USER COMMAND PID FD PROTO LOCAL FOREIGN. Anchored on PROTO being in the
    // fifth column, which `netstat -an` (where "tcp4" leads the line) never satisfies.
    if cols.len() >= 6 && matches!(cols[4], "tcp" | "tcp4" | "tcp6" | "tcp46") {
        return cols.get(2).and_then(|c| c.parse().ok());
    }
    None
}

/// Listening TCP ports that look like a database, from whichever probe succeeded.
fn parse_listeners(text: &str) -> Vec<Listener> {
    let mut found: Vec<Listener> = Vec::new();
    for line in text.lines() {
        // Rather than depend on column positions (which differ between every one of the
        // five tools), scan every token for an address whose port we care about.
        let mut hit = None;
        for token in line.split_whitespace() {
            if let Some((host, p)) = split_addr_port(token) {
                if product_for_port(p).is_some() {
                    hit = Some((host, p));
                    break;
                }
            }
        }
        let Some((host, port)) = hit else { continue };
        if found.iter().any(|l| l.port == port) {
            continue;
        }
        found.push(Listener {
            port,
            pid: pid_from_line(line),
            addr: (!is_wildcard_or_loopback(host)).then(|| host.to_string()),
        });
    }
    found.sort_by_key(|l| l.port);
    found
}

struct DockerContainer {
    /// Full (untruncated) id, for matching against a cgroup path.
    id: String,
    name: String,
    image: String,
    /// Host-side ports this container publishes.
    published: Vec<u16>,
    /// The container-side port, when the mapping revealed one.
    container_port: Option<u16>,
}

/// Parse `docker ps --no-trunc --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Ports}}'`.
fn parse_docker(text: &str) -> Vec<DockerContainer> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(id), Some(name), Some(image)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let ports = cols.next().unwrap_or("");
        let (published, container_port) = parse_port_map(ports);
        out.push(DockerContainer {
            id: id.trim().to_string(),
            name: name.trim().to_string(),
            image: image.trim().to_string(),
            published,
            container_port,
        });
    }
    out
}

/// Parse a Docker ports column, e.g. `0.0.0.0:3306->3306/tcp, :::3306->3306/tcp`.
fn parse_port_map(text: &str) -> (Vec<u16>, Option<u16>) {
    let mut published = Vec::new();
    let mut container_port = None;
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once("->") {
            Some((host_side, container_side)) => {
                // Host side is `addr:port`; take what follows the last colon.
                if let Some((_, p)) = host_side.rsplit_once(':') {
                    if let Ok(port) = p.trim().parse::<u16>() {
                        if !published.contains(&port) {
                            published.push(port);
                        }
                    }
                }
                // Container side is `port/proto`.
                let cp = container_side.split('/').next().unwrap_or("").trim();
                if let Ok(port) = cp.parse::<u16>() {
                    container_port = Some(port);
                }
            }
            // Exposed but not published, e.g. `3306/tcp`.
            None => {
                let cp = part.split('/').next().unwrap_or("").trim();
                if let Ok(port) = cp.parse::<u16>() {
                    container_port = Some(port);
                }
            }
        }
    }
    (published, container_port)
}

/// Parse the `docker inspect` name/IP pairs. Names come back with a leading `/`.
fn parse_container_ips(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(name), Some(ips)) = (cols.next(), cols.next()) else {
            continue;
        };
        // A container can be on several networks; the first usable address will do,
        // since all of them are routable from the SSH host.
        if let Some(ip) = ips.split_whitespace().find(|s| !s.is_empty()) {
            out.push((name.trim_start_matches('/').to_string(), ip.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_native_listener() {
        let out = "@@LISTEN\n\
LISTEN 0 151 127.0.0.1:3306 0.0.0.0:* users:((\"mariadbd\",pid=900,fd=20))\n\
LISTEN 0 4096 0.0.0.0:22 0.0.0.0:*\n\
@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, DbKind::Native);
        assert_eq!(found[0].port, 3306);
        assert_eq!(found[0].host, "127.0.0.1");
    }

    #[test]
    fn ignores_unrelated_listeners() {
        let out = "@@LISTEN\nLISTEN 0 4096 0.0.0.0:22 0.0.0.0:*\nLISTEN 0 511 *:80 *:*\n@@DOCKER\n@@DOCKERIP\n";
        assert!(parse(out).is_empty());
    }

    #[test]
    fn finds_a_published_docker_database() {
        let out = "@@LISTEN\n\
LISTEN 0 4096 0.0.0.0:3306 0.0.0.0:*\n\
@@DOCKER\n\
cid-db-main\tdb-main\tmariadb:11\t0.0.0.0:3306->3306/tcp, :::3306->3306/tcp\n\
@@DOCKERIP\n/db-main\t172.17.0.4 \n";
        let found = parse(out);
        // The host listener is the container's published port — one entry, not two.
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, DbKind::Docker);
        assert_eq!(found[0].host, "127.0.0.1");
        assert_eq!(found[0].port, 3306);
        assert_eq!(found[0].container.as_deref(), Some("db-main"));
    }

    #[test]
    fn reaches_an_unpublished_container_over_the_bridge() {
        let out = "@@LISTEN\n@@DOCKER\n\
cid-hidden-db\thidden-db\tmysql:8\t3306/tcp\n\
@@DOCKERIP\n/hidden-db\t172.18.0.7 \n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        // No published port, so it must route via the container address.
        assert_eq!(found[0].host, "172.18.0.7");
        assert_eq!(found[0].port, 3306);
    }

    #[test]
    fn skips_an_unpublished_container_with_no_known_address() {
        let out = "@@LISTEN\n@@DOCKER\nghost\tmysql:8\t3306/tcp\n@@DOCKERIP\n";
        assert!(parse(out).is_empty(), "not reachable — must not be offered");
    }

    #[test]
    fn finds_a_postgres_container_bound_to_loopback() {
        // The exact `docker ps` shape that was being missed: a Postgres container
        // published on 127.0.0.1 rather than 0.0.0.0. Discovery originally only knew
        // MySQL ports and images, so this was filtered out entirely.
        let out = "@@LISTEN\n\
LISTEN 0 4096 127.0.0.1:5432 0.0.0.0:*\n\
@@DOCKER\n\
cid-olds_studio-postgres-1\tolds_studio-postgres-1\tpostgres:16-alpine\t127.0.0.1:5432->5432/tcp\n\
@@DOCKERIP\n/olds_studio-postgres-1\t172.19.0.2 \n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].engine, Some(DbEngine::Postgres));
        assert_eq!(found[0].kind, DbKind::Docker);
        assert_eq!(found[0].port, 5432);
        assert_eq!(found[0].host, "127.0.0.1");
        assert_eq!(found[0].container.as_deref(), Some("olds_studio-postgres-1"));
    }

    #[test]
    fn recognises_postgres_forks_and_a_remapped_port() {
        // pgvector/timescale are Postgres under another name, and a host port remapped
        // away from 5432 must still work — the image is the authority, not the port.
        let out = "@@LISTEN\n@@DOCKER\n\
cid-vec\tvec\tpgvector/pgvector:pg16\t0.0.0.0:15432->5432/tcp\n\
@@DOCKERIP\n/vec\t172.20.0.3 \n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].engine, Some(DbEngine::Postgres));
        assert_eq!(found[0].port, 15432, "should use the published host port");
    }

    #[test]
    fn finds_a_native_postgres_listener() {
        let out = "@@LISTEN\nLISTEN 0 244 127.0.0.1:5432 0.0.0.0:*\n@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].engine, Some(DbEngine::Postgres));
        assert_eq!(found[0].kind, DbKind::Native);
    }

    #[test]
    fn mysql_and_postgres_on_one_host_are_both_listed() {
        let out = "@@LISTEN\n\
LISTEN 0 151 127.0.0.1:3306 0.0.0.0:*\n\
LISTEN 0 244 127.0.0.1:5432 0.0.0.0:*\n\
@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|e| e.engine == Some(DbEngine::MySql)));
        assert!(found.iter().any(|e| e.engine == Some(DbEngine::Postgres)));
    }

    #[test]
    fn a_host_networked_container_is_attributed_to_its_container() {
        // Taken verbatim from a real server: MariaDB runs in a container started with
        // host networking, so `mariadbd` listens on the HOST's 127.0.0.1:3306 and
        // `docker ps` shows no port mapping for it at all. Treating that as a host
        // install made the query layer run `mysql` on the host — which has no mysql
        // client installed — and every connection failed.
        let out = "@@LISTEN\n\
LISTEN 0 80 127.0.0.1:3306 0.0.0.0:* users:((\"mariadbd\",pid=1133344,fd=29))\n\
@@DOCKER\n\
475abd3d79ab712e10c7f2e3e84dac4b660cea48998c37fd5e8084d9ac4d6c99\tm2boot\tmetin2-run:phaseB\t\n\
@@DOCKERIP\n\
@@PIDCG\n\
1133344\t0::/system.slice/docker-475abd3d79ab712e10c7f2e3e84dac4b660cea48998c37fd5e8084d9ac4d6c99.scope \n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, DbKind::Docker, "must be reachable via docker exec");
        assert_eq!(found[0].container.as_deref(), Some("m2boot"));
        // Host-networked, so the container's loopback is the host's.
        assert_eq!(found[0].host, "127.0.0.1");
        assert_eq!(found[0].port, 3306);
        assert_eq!(found[0].engine, Some(DbEngine::MySql));
    }

    #[test]
    fn a_listener_with_no_owning_container_is_still_native() {
        // A genuine host install has no container cgroup, and must not be mislabelled.
        let out = "@@LISTEN\n\
LISTEN 0 80 127.0.0.1:3306 0.0.0.0:* users:((\"mariadbd\",pid=999,fd=29))\n\
@@DOCKER\n@@DOCKERIP\n@@PIDCG\n\
999\t0::/system.slice/mariadb.service \n";
        let found = parse(out);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].kind, DbKind::Native);
        assert_eq!(found[0].container, None);
    }

    #[test]
    fn cgroup_v1_layout_is_also_understood() {
        let out = "@@LISTEN\n\
LISTEN 0 80 127.0.0.1:5432 0.0.0.0:* users:((\"postgres\",pid=42,fd=7))\n\
@@DOCKER\n\
abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789\tpg\tpostgres:16\t\n\
@@DOCKERIP\n@@PIDCG\n\
42\t9:cpu:/docker/abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789 \n";
        let found = parse(out);
        assert_eq!(found[0].container.as_deref(), Some("pg"), "{found:?}");
    }

    #[test]
    fn finds_every_common_database_product() {
        let out = "@@LISTEN\n@@DOCKER\n\
cid-redis-cache\tredis-cache\tredis:7-alpine\t0.0.0.0:6379->6379/tcp\n\
cid-mongo-main\tmongo-main\tmongo:7\t0.0.0.0:27017->27017/tcp\n\
cid-mssql\tmssql\tmcr.microsoft.com/mssql/server:2022-latest\t0.0.0.0:1433->1433/tcp\n\
cid-ch\tch\tclickhouse/clickhouse-server\t0.0.0.0:8123->8123/tcp\n\
cid-es\tes\telasticsearch:8.13.0\t0.0.0.0:9200->9200/tcp\n\
@@DOCKERIP\n";
        let found = parse(out);
        let products: Vec<DbProduct> = found.iter().map(|e| e.product).collect();
        for want in [
            DbProduct::Redis,
            DbProduct::MongoDb,
            DbProduct::MsSql,
            DbProduct::ClickHouse,
            DbProduct::Elasticsearch,
        ] {
            assert!(products.contains(&want), "missing {want:?} in {products:?}");
        }
    }

    #[test]
    fn browsable_products_carry_an_engine_and_the_rest_are_listed_without_one() {
        let out = "@@LISTEN\n@@DOCKER\n\
cid-r\tr\tredis:7\t0.0.0.0:6379->6379/tcp\n\
cid-m\tm\tmongo:7\t0.0.0.0:27017->27017/tcp\n\
@@DOCKERIP\n";
        let found = parse(out);
        let redis = found.iter().find(|e| e.product == DbProduct::Redis).unwrap();
        let mongo = found.iter().find(|e| e.product == DbProduct::MongoDb).unwrap();
        // Redis can be opened; Mongo is discovered but not yet browsable, and says so
        // rather than offering a sign-in that would fail.
        assert_eq!(redis.engine, Some(DbEngine::Redis));
        assert_eq!(mongo.engine, None);
    }

    #[test]
    fn valkey_and_scylla_are_recognised_as_their_originals() {
        let out = "@@LISTEN\n@@DOCKER\n\
cid-v\tv\tvalkey/valkey:8\t0.0.0.0:6379->6379/tcp\n\
cid-s\ts\tscylladb/scylla\t0.0.0.0:9042->9042/tcp\n\
@@DOCKERIP\n";
        let found = parse(out);
        assert!(found.iter().any(|e| e.product == DbProduct::Redis));
        assert!(found.iter().any(|e| e.product == DbProduct::Cassandra));
    }

    #[test]
    fn ignores_non_database_containers() {
        let out = "@@LISTEN\n@@DOCKER\nweb\tnginx:latest\t0.0.0.0:80->80/tcp\n@@DOCKERIP\n/web\t172.17.0.2 \n";
        assert!(parse(out).is_empty());
    }

    #[test]
    fn survives_a_host_without_docker_or_ss() {
        assert!(parse("@@LISTEN\n@@DOCKER\n@@DOCKERIP\n").is_empty());
        assert!(parse("").is_empty());
    }

    /// Build a discovery blob from lines, so no escape sequence has to survive being
    /// written out by a tool.
    fn out(lines: &[&str]) -> String {
        let nl = String::from_utf8(vec![10]).unwrap();
        lines.join(&nl) + &nl
    }

    /// FreeBSD has no `ss`, and its `netstat` rejects the Linux `-ltnp` flags, so the
    /// listener section came back empty and nothing was ever found. `sockstat` is the
    /// FreeBSD equivalent, and it reports a PID too.
    #[test]
    fn finds_a_freebsd_listener_from_sockstat() {
        let found = parse(&out(&[
            "@@OS", "FreeBSD", "@@LISTEN",
            "USER     COMMAND    PID   FD PROTO  LOCAL ADDRESS     FOREIGN ADDRESS",
            "mysql    mysqld     1234  30 tcp4   127.0.0.1:3306    *:*",
            "root     sshd        901   4 tcp4   *:22              *:*",
            "@@DOCKER", "@@DOCKERIP",
        ]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].port, 3306);
        assert_eq!(found[0].kind, DbKind::Native);
        assert_eq!(found[0].engine, Some(DbEngine::MySql));
    }

    /// The other half of the FreeBSD problem: the BSDs and Solaris write `addr.port`, not
    /// `addr:port`, so even a probe that worked produced no matches.
    #[test]
    fn understands_the_bsd_dotted_address_form() {
        let found = parse(&out(&[
            "@@OS", "FreeBSD", "@@LISTEN",
            "tcp4       0      0 127.0.0.1.3306    *.*      LISTEN",
            "tcp4       0      0 *.22              *.*      LISTEN",
            "@@DOCKER", "@@DOCKERIP",
        ]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].port, 3306);
        assert_eq!(found[0].host, "127.0.0.1");
    }

    #[test]
    fn finds_a_macos_listener_from_lsof() {
        let found = parse(&out(&[
            "@@OS", "Darwin", "@@LISTEN",
            "postgres  742 you    7u  IPv4 0x1234  0t0  TCP 127.0.0.1:5432 (LISTEN)",
            "@@DOCKER", "@@DOCKERIP",
        ]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].engine, Some(DbEngine::Postgres));
    }

    /// A database bound to one specific address — a jail IP being the usual reason — has
    /// to be reached at that address. Forwarding it to loopback connects to nothing.
    #[test]
    fn a_listener_bound_to_one_address_keeps_that_address() {
        let found = parse(&out(&[
            "@@OS", "FreeBSD", "@@LISTEN",
            "tcp4       0      0 10.0.0.5.3306     *.*      LISTEN",
            "@@DOCKER", "@@DOCKERIP",
        ]));
        assert_eq!(found[0].host, "10.0.0.5", "{found:?}");
    }

    /// Wildcard and loopback binds still go to loopback: it reaches them, and which of the
    /// host addresses we pick does not matter.
    #[test]
    fn a_wildcard_bind_still_uses_loopback() {
        for line in [
            "tcp4       0      0 *.3306         *.*      LISTEN",
            "tcp4       0      0 0.0.0.0.3306   *.*      LISTEN",
            "LISTEN 0 151 0.0.0.0:3306 0.0.0.0:*",
        ] {
            let found = parse(&out(&["@@OS", "X", "@@LISTEN", line, "@@DOCKER", "@@DOCKERIP"]));
            assert_eq!(found.len(), 1, "{line}");
            assert_eq!(found[0].host, "127.0.0.1", "{line}");
        }
    }

    /// The floor of the probe chain: plain `netstat -an` names no process, and that still
    /// has to yield a usable endpoint rather than nothing.
    #[test]
    fn plain_netstat_with_no_process_column_still_works() {
        let found = parse(&out(&[
            "@@OS", "SunOS", "@@LISTEN",
            "      *.5432               *.*                0      0 128000      0 LISTEN",
            "@@DOCKER", "@@DOCKERIP",
        ]));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].engine, Some(DbEngine::Postgres));
    }

    /// `netstat -an` leads its lines with "tcp4", which must not be mistaken for
    /// sockstat's PROTO column and read as a PID — that would attribute the socket to
    /// whichever unrelated process happens to hold that id.
    #[test]
    fn a_netstat_line_is_not_misread_as_a_sockstat_pid() {
        assert_eq!(
            pid_from_line("tcp4       0      0 127.0.0.1.3306    *.*      LISTEN"),
            None
        );
        assert_eq!(
            pid_from_line("mysql    mysqld     1234  30 tcp4   127.0.0.1:3306   *:*"),
            Some(1234)
        );
        assert_eq!(
            pid_from_line("postgres  742 you 7u IPv4 0x1 0t0 TCP 127.0.0.1:5432 (LISTEN)"),
            Some(742)
        );
        assert_eq!(
            pid_from_line(r#"LISTEN 0 80 127.0.0.1:3306 0.0.0.0:* users:(("mariadbd",pid=1133344,fd=29))"#),
            Some(1133344)
        );
    }

    /// An IPv6 address is full of both colons and dots; splitting at the wrong one invents
    /// a port out of part of the address.
    #[test]
    fn ipv6_addresses_split_at_the_right_separator() {
        assert_eq!(split_addr_port("[::1]:5432"), Some(("[::1]", 5432)));
        assert_eq!(split_addr_port("::1.5432"), Some(("::1", 5432)));
        assert_eq!(split_addr_port("*:*"), None);
        assert_eq!(split_addr_port("*.*"), None);
    }

    #[test]
    fn handles_a_nonstandard_port() {
        let out = "@@LISTEN\nLISTEN 0 151 127.0.0.1:3307 0.0.0.0:*\n@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].port, 3307);
    }

}
