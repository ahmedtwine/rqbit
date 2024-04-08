mod s3_client;

#[tokio::main]
async fn main() {
    s3_client::download_s3_video().await;
    println!("Hello, world!");
}
