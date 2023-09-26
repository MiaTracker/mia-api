#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    api::launch().await;
}
