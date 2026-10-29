//! One Postgres container per test binary, one fresh database per test.
//!
//! The first `database()` call in a binary starts a Postgres container, points the global config
//! at its `mia_template` database (migration `m20260417_000002_add_still_image_type` opens its own
//! connection from `config().db.connection_url`), migrates that template once and closes every
//! connection to it. Each call then clones the template with `CREATE DATABASE ... TEMPLATE`, so
//! every test gets an isolated, already migrated database.
//!
//! The template is migrated one migration at a time. Applying them all in one `Migrator::up` call
//! fails on an empty database (a known bug, see `postgres_migrations_in_one_run.rs`).
//!
//! testcontainers-rs has no Ryuk reaper and a static is never dropped, so an `atexit` hook removes
//! the container when the test process exits (the `watchdog` feature covers Ctrl-C).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use infrastructure::{Configuration, DbConfiguration};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use services::infrastructure::constants::init_tmdb_config_once;
use services::test_support::config::{test_config, test_tmdb_configuration};
use testcontainers::runners::SyncRunner;
use testcontainers::{Container, ImageExt};
use testcontainers_modules::postgres::Postgres;

const POSTGRES_TAG: &str = "18-alpine";
const TEMPLATE: &str = "mia_template";
const EMPTY: &str = "mia_empty";

struct Server {
    host: String,
    port: u16,
}

impl Server {
    fn url(&self, database: &str) -> String {
        format!("postgres://postgres:postgres@{}:{}/{}", self.host, self.port, database)
    }
}

/// The bootstrap's outcome. A failure is recorded, so every later test in the binary fails fast
/// with the same message instead of starting another container and migrating again.
static SERVER: OnceLock<Result<Server, String>> = OnceLock::new();
static CONTAINER: Mutex<Option<Container<Postgres>>> = Mutex::new(None);
static NEXT_DATABASE: AtomicUsize = AtomicUsize::new(0);
/// Clones of the template are made one at a time.
static CREATE_DATABASE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A fresh, migrated database for one test.
pub async fn database() -> DatabaseConnection {
    let server = server(TEMPLATE, |server| {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("bootstrap runtime");
        runtime.block_on(create_template(server));
    });

    let name = format!("test_{}", NEXT_DATABASE.fetch_add(1, Ordering::SeqCst));
    {
        let _guard = CREATE_DATABASE.lock().await;
        let admin = connect(&server.url("postgres")).await;
        admin.execute_unprepared(&format!(r#"CREATE DATABASE "{name}" TEMPLATE "{TEMPLATE}""#)).await
            .unwrap_or_else(|err| panic!("creating database {name}: {err}"));
        admin.close().await.expect("closing admin connection");
    }

    connect(&server.url(&name)).await
}

/// An empty database (no migrations applied) that the global config points at, as
/// `cargo run -- migrate up` would see it. Call it at most once per test binary, and not together
/// with `database()`.
pub async fn configured_empty_database() -> DatabaseConnection {
    let server = server(EMPTY, |_| {});

    let admin = connect(&server.url("postgres")).await;
    admin.execute_unprepared(&format!(r#"CREATE DATABASE "{EMPTY}""#)).await.expect("creating the empty database");
    admin.close().await.expect("closing admin connection");

    connect(&server.url(EMPTY)).await
}

/// The binary's server, started on first use with the global config pointing at `configured`,
/// and `prepare` run once after that.
fn server(configured: &'static str, prepare: fn(&Server)) -> &'static Server {
    let server = SERVER.get_or_init(|| {
        // Off the test's runtime: the sync runner and `prepare` block on runtimes of their own,
        // which would panic inside the test's.
        std::thread::spawn(move || {
            let server = start_container();
            configure(&server, configured);
            prepare(&server);
            server
        }).join().map_err(|panic| panic_message(panic.as_ref()))
    }).as_ref().unwrap_or_else(|message| panic!("Postgres bootstrap failed: {message}"));
    assert_eq!(infrastructure::config().db.connection_url, server.url(configured),
               "this binary's config points at another database; database() and configured_empty_database() can't be mixed");
    server
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic.downcast_ref::<&str>().map(|message| message.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

fn configure(server: &Server, database: &str) {
    let url = server.url(database);
    let configured = infrastructure::init_config_once(|| Configuration {
        db: DbConfiguration { connection_url: url.clone(), schema: "public".into() },
        ..test_config()
    });
    assert_eq!(configured.db.connection_url, url,
               "the global config was initialised before the Postgres bootstrap; don't call test_support::init() in Postgres tests");
    init_tmdb_config_once(&test_tmdb_configuration());
}

fn start_container() -> Server {
    let container = Postgres::default().with_tag(POSTGRES_TAG).start()
        .expect("starting the Postgres container (is Docker running?)");
    let server = Server {
        host: container.get_host().expect("container host").to_string(),
        port: container.get_host_port_ipv4(5432).expect("container port"),
    };
    *CONTAINER.lock().expect("container lock") = Some(container);
    // SAFETY: registers a plain `extern "C"` function with no preconditions.
    unsafe { libc::atexit(remove_container) };
    server
}

extern "C" fn remove_container() {
    let _ = std::panic::catch_unwind(|| {
        if let Ok(mut container) = CONTAINER.lock() {
            drop(container.take());
        }
    });
}

async fn create_template(server: &Server) {
    let admin = connect(&server.url("postgres")).await;
    admin.execute_unprepared(&format!(r#"CREATE DATABASE "{TEMPLATE}""#)).await.expect("creating the template database");

    let template = connect(&server.url(TEMPLATE)).await;
    // One migration (and one transaction) at a time.
    while !Migrator::get_pending_migrations(&template).await.expect("listing pending migrations").is_empty() {
        Migrator::up(&template, Some(1)).await.unwrap_or_else(|err| panic!("migrating the template database: {err}"));
    }
    template.close().await.expect("closing the template connection");

    // A migration opened a connection of its own. Make sure nothing stays connected (a template
    // with connections can't be cloned), and that nothing connects later.
    admin.execute_unprepared(&format!(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{TEMPLATE}' AND pid <> pg_backend_pid()"
    )).await.expect("disconnecting from the template database");
    admin.execute_unprepared(&format!(r#"ALTER DATABASE "{TEMPLATE}" WITH ALLOW_CONNECTIONS false"#)).await
        .expect("closing the template database to connections");
    admin.close().await.expect("closing admin connection");
}

async fn connect(url: &str) -> DatabaseConnection {
    let mut options = ConnectOptions::new(url);
    options.max_connections(5).acquire_timeout(Duration::from_secs(30)).sqlx_logging(false);
    Database::connect(options).await.unwrap_or_else(|err| panic!("connecting to {url}: {err}"))
}
