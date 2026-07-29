//! Find the database servers running on a host.
//!
//! Two places they hide, and they need different treatment:
//!
//! - **Natively installed** — listening on the host's own network stack. `ss -ltnp`
//!   reports those, and a tunnel to `127.0.0.1:<port>` reaches them even when the port
//!   is firewalled off from the internet (which it should be).
//! - **Inside Docker** — a container may publish a port to the host, or may only be
//!   reachable on the bridge network. `docker ps` gives the published mapping;
//!   `docker inspect` gives the container IP for the ones that publish nothing. Because
//!   the tunnel's destination is resolved by the SSH server, `172.17.0.x:3306` works
//!   from here just as well as a published port — no `docker exec`, no published port
//!   required, and no changes to how the user runs their containers.
//!
//! Everything is parsed into typed records rather than fed to a model as text, so the UI
//! can list candidates and the user just picks one.

use serde::Serialize;

use crate::ssh::{shell_quote, SessionManager};

/// Ports worth treating as "probably a database" when scanning listeners.
const MYSQL_PORTS: &[u16] = &[3306, 3307, 3308, 33060];

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
    /// Container image, when known — a good hint at MySQL vs MariaDB.
    pub image: Option<String>,
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
const DISCOVER_CMD: &str = "\
echo '@@LISTEN'; (ss -ltnp 2>/dev/null || netstat -ltnp 2>/dev/null) || true; \
echo '@@DOCKER'; (docker ps --format '{{.Names}}\t{{.Image}}\t{{.Ports}}' 2>/dev/null) || true; \
echo '@@DOCKERIP'; (docker ps -q 2>/dev/null | xargs -r docker inspect \
 -f '{{.Name}}\t{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}' 2>/dev/null) || true";

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
    Listen,
    Docker,
    DockerIp,
}

/// Split the combined output into its sections and parse each.
///
/// Sections are collected into a fixed array indexed by the marker rather than by
/// juggling `&mut` references to three separate locals — same result, but nothing for a
/// reader (or the borrow checker) to puzzle over.
fn parse(stdout: &str) -> Vec<DbEndpoint> {
    let mut sections: [String; 3] = [String::new(), String::new(), String::new()];
    let mut current = Section::Listen;

    for line in stdout.lines() {
        match line.trim() {
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
            _ => {}
        }
        let slot = &mut sections[current as usize];
        slot.push_str(line);
        slot.push('\n');
    }

    let [listen, docker, docker_ip] = sections;
    let ips = parse_container_ips(&docker_ip);
    let mut found: Vec<DbEndpoint> = Vec::new();

    // Docker first: a container that publishes 3306 also shows up as a host listener,
    // and the container record is the more informative of the two.
    let mut published: Vec<u16> = Vec::new();
    for c in parse_docker(&docker) {
        let is_db = looks_like_db(&c.image) || c.published.iter().any(|p| is_db_port(*p));
        if !is_db {
            continue;
        }
        // Prefer a published host port (no bridge routing needed); fall back to the
        // container's own address on the Docker network.
        let (host, port) = match c.published.iter().copied().find(|p| is_db_port(*p)) {
            Some(p) => {
                published.push(p);
                ("127.0.0.1".to_string(), p)
            }
            None => match ips.iter().find(|(name, _)| *name == c.name) {
                Some((_, ip)) => (ip.clone(), c.container_port.unwrap_or(3306)),
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
        });
    }

    // Then host listeners that no container already accounted for.
    for port in parse_listeners(&listen) {
        if published.contains(&port) || found.iter().any(|e| e.port == port) {
            continue;
        }
        found.push(DbEndpoint {
            id: format!("native:{port}"),
            label: if port == 3306 {
                "MySQL / MariaDB (installed on the host)".to_string()
            } else {
                format!("Database on port {port} (installed on the host)")
            },
            kind: DbKind::Native,
            host: "127.0.0.1".to_string(),
            port,
            container: None,
            image: None,
        });
    }

    found
}

fn is_db_port(port: u16) -> bool {
    MYSQL_PORTS.contains(&port)
}

fn looks_like_db(image: &str) -> bool {
    let i = image.to_ascii_lowercase();
    i.contains("mysql") || i.contains("mariadb") || i.contains("percona")
}

/// Listening TCP ports that look like a database, from `ss -ltnp` / `netstat -ltnp`.
fn parse_listeners(text: &str) -> Vec<u16> {
    let mut ports = Vec::new();
    for line in text.lines() {
        // The local address is the 4th column for `ss`, 4th for `netstat` too; rather
        // than depend on column counts (which differ between the two and across
        // versions), scan every token for a `host:port` ending in a port we care about.
        for token in line.split_whitespace() {
            if let Some((_, tail)) = token.rsplit_once(':') {
                if let Ok(port) = tail.parse::<u16>() {
                    if is_db_port(port) && !ports.contains(&port) {
                        ports.push(port);
                    }
                }
            }
        }
    }
    ports.sort_unstable();
    ports
}

struct DockerContainer {
    name: String,
    image: String,
    /// Host-side ports this container publishes.
    published: Vec<u16>,
    /// The container-side port, when the mapping revealed one.
    container_port: Option<u16>,
}

/// Parse `docker ps --format '{{.Names}}\t{{.Image}}\t{{.Ports}}'`.
fn parse_docker(text: &str) -> Vec<DockerContainer> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(name), Some(image)) = (cols.next(), cols.next()) else {
            continue;
        };
        let ports = cols.next().unwrap_or("");
        let (published, container_port) = parse_port_map(ports);
        out.push(DockerContainer {
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

/// Build the `mysql` CLI probe used to confirm credentials without a driver. Kept here
/// so the quoting lives next to everything else that shells out.
pub fn version_probe(container: Option<&str>, user: &str, password: &str) -> String {
    let inner = format!(
        "mysql -u {} -p{} -N -B -e 'select version()'",
        shell_quote(user),
        shell_quote(password),
    );
    match container {
        Some(c) => format!("docker exec -i {} sh -lc {}", shell_quote(c), shell_quote(&inner)),
        None => inner,
    }
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
db-main\tmariadb:11\t0.0.0.0:3306->3306/tcp, :::3306->3306/tcp\n\
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
hidden-db\tmysql:8\t3306/tcp\n\
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
    fn ignores_non_database_containers() {
        let out = "@@LISTEN\n@@DOCKER\nweb\tnginx:latest\t0.0.0.0:80->80/tcp\n@@DOCKERIP\n/web\t172.17.0.2 \n";
        assert!(parse(out).is_empty());
    }

    #[test]
    fn survives_a_host_without_docker_or_ss() {
        assert!(parse("@@LISTEN\n@@DOCKER\n@@DOCKERIP\n").is_empty());
        assert!(parse("").is_empty());
    }

    #[test]
    fn handles_a_nonstandard_port() {
        let out = "@@LISTEN\nLISTEN 0 151 127.0.0.1:3307 0.0.0.0:*\n@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].port, 3307);
    }

    #[test]
    fn version_probe_quotes_credentials() {
        let cmd = version_probe(None, "root", "p'wd; rm -rf /");
        assert!(cmd.contains(r#"'p'\''wd; rm -rf /'"#), "got: {cmd}");
        let in_container = version_probe(Some("db main"), "root", "x");
        assert!(in_container.starts_with("docker exec -i 'db main'"), "got: {in_container}");
    }
}
