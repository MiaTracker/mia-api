use std::env;
use infrastructure::config;
use migration::{migrate_down, migrate_fresh, migrate_refresh, migrate_reset, migrate_status, migrate_up};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    infrastructure::initialize().await;

    tracing_subscriber::fmt()
        .with_max_level(config().logging.level.clone())
        .init();

    let mut args = env::args().skip(1);

    if let Some(arg) = args.next() {
        match arg.as_str() {
            "serve" => {
                api::launch().await;
            },
            "refresh" => {
                services::refresh::run_refresh().await;
            },
            "linearize-ratings" => {
                let mut user_uuid: Option<Uuid> = None;
                let mut dry_run = false;
                while let Some(arg) = args.next() {
                    match arg.as_str() {
                        "--user" => {
                            let raw = args.next().expect("--user requires a value");
                            user_uuid = Some(Uuid::parse_str(&raw).expect("invalid uuid"));
                        }
                        "--dry-run" => dry_run = true,
                        x => println!("Invalid flag for linearize-ratings: {}", x),
                    }
                }
                services::ratings_linearization::run_linearize_ratings(user_uuid, dry_run).await;
            },
            "migrate" => {
                if let Some(arg) = args.next() {
                    match arg.as_str() {
                        "up" => {
                            if let Some(num) = args.next() {
                                let parse = num.parse::<u32>();
                                match parse {
                                    Ok(num) => { migrate_up(Some(num)).await; }
                                    Err(_) => { println!("Invalid number: {}", num); }
                                }
                            } else {
                                migrate_up(None).await;
                            }
                        },
                        "down" => {
                            if let Some(num) = args.next() {
                                let parse = num.parse::<u32>();
                                match parse {
                                    Ok(num) => { migrate_down(Some(num)).await; }
                                    Err(_) => { println!("Invalid number: {}", num); }
                                }
                            } else {
                                migrate_down(None).await;
                            }
                        },
                        "status" => {
                            migrate_status().await;
                        },
                        "fresh" => {
                            migrate_fresh().await;
                        },
                        "refresh" => {
                            migrate_refresh().await;
                        },
                        "reset" => {
                            migrate_reset().await;
                        },
                        x => {
                            println!("Invalid subcommand: {}", x);
                        }
                    }
                }
            },
            x => {
                println!("Invalid command: {}", x);
            }
        }
    }
}
