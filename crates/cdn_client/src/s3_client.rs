use aws_config::BehaviorVersion;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::{error::SdkError, Client};
use std::str;

use tokio::io::AsyncReadExt;

async fn download_object(
    client: &Client,
    bucket_name: &str,
    key: &str,
) -> Result<(), SdkError<GetObjectError>> {
    let mut stream = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?
        .body
        .into_async_read();

    let mut buffer = Vec::new();

    // Read data into buffer
    while stream.read_buf(&mut buffer).await.unwrap() != 0 {
        // Print the buffer bytes for demonstration purposes
        println!("Buffer: {:?}", buffer);
        // Process the buffer bytes as needed (e.g., write to a file, send to a video player, etc.)
        buffer.clear();
    }

    Ok(())
}

pub async fn download_s3_video() -> () {
    let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let client = aws_sdk_s3::Client::new(&config);
    let bucket_name = "bacbone-assets";
    download_object(&client, bucket_name, "file_example_MP4_480_1_5MG.mp4")
        .await
        .unwrap();
}
