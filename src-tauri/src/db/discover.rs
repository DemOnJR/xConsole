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
    for port in parse_listeners(&listen) {
        if published.contains(&port) || found.iter().any(|e| e.port == port) {
            continue;
        }
        let Some(product) = product_for_port(port) else {
            continue;
        };
        found.push(DbEndpoint {
            id: format!("native:{port}"),
            label: format!("{} on the host (port {port})", product.label()),
            kind: DbKind::Native,
            host: "127.0.0.1".to_string(),
            port,
            container: None,
            image: None,
            product,
            engine: product.engine(),
        });
    }

    found
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
                    if product_for_port(port).is_some() && !ports.contains(&port) {
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
    fn finds_a_postgres_container_bound_to_loopback() {
        // The exact `docker ps` shape that was being missed: a Postgres container
        // published on 127.0.0.1 rather than 0.0.0.0. Discovery originally only knew
        // MySQL ports and images, so this was filtered out entirely.
        let out = "@@LISTEN\n\
LISTEN 0 4096 127.0.0.1:5432 0.0.0.0:*\n\
@@DOCKER\n\
olds_studio-postgres-1\tpostgres:16-alpine\t127.0.0.1:5432->5432/tcp\n\
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
vec\tpgvector/pgvector:pg16\t0.0.0.0:15432->5432/tcp\n\
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
    fn finds_every_common_database_product() {
        let out = "@@LISTEN\n@@DOCKER\n\
redis-cache\tredis:7-alpine\t0.0.0.0:6379->6379/tcp\n\
mongo-main\tmongo:7\t0.0.0.0:27017->27017/tcp\n\
mssql\tmcr.microsoft.com/mssql/server:2022-latest\t0.0.0.0:1433->1433/tcp\n\
ch\tclickhouse/clickhouse-server\t0.0.0.0:8123->8123/tcp\n\
es\telasticsearch:8.13.0\t0.0.0.0:9200->9200/tcp\n\
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
r\tredis:7\t0.0.0.0:6379->6379/tcp\n\
m\tmongo:7\t0.0.0.0:27017->27017/tcp\n\
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
v\tvalkey/valkey:8\t0.0.0.0:6379->6379/tcp\n\
s\tscylladb/scylla\t0.0.0.0:9042->9042/tcp\n\
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

    #[test]
    fn handles_a_nonstandard_port() {
        let out = "@@LISTEN\nLISTEN 0 151 127.0.0.1:3307 0.0.0.0:*\n@@DOCKER\n@@DOCKERIP\n";
        let found = parse(out);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].port, 3307);
    }

}
