// Declare the s3_client module
pub mod client;
pub mod s3_client;

// Re-export the download_s3_video function
pub use s3_client::download_object;
pub use s3_client::get_aws_client_bucket;

#[tokio::main]
async fn main() {
    // let (client, bucket_name) = get_aws_client_bucket().await;
    // download_object(&client, bucket_name, "file_example_MP4_480_1_5MG.mp4", None)
    //     .await
    //     .unwrap();

    s3_client::download_stream_public_url(
        "https://bacbone-assets.s3.us-west-2.amazonaws.com/file_example_MP4_480_1_5MG.mp4",
        "file_output.mp4",
    )
    .await
    .unwrap();
}
