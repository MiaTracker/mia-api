use std::env;
use infrastructure::config;
use migration::{migrate_down, migrate_fresh, migrate_refresh, migrate_reset, migrate_status, migrate_up};

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
