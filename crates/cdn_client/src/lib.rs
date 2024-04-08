// Declare the s3_client module
pub mod s3_client;

// Re-export the download_s3_video function
pub use s3_client::download_object;
pub use s3_client::get_aws_client_bucket;

// #[tokio::main]
// async fn main() {
//     s3_client::download_s3_video().await;
//     println!("Hello, world!");
// }
