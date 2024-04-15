use super::s3_client;
use lava_torrent::torrent::v1::TorrentBuilder;
use librqbit::{torrent_from_bytes, ByteBufOwned, TorrentMetaV1};
use librqbit::{
    torrent_state::ManagedTorrentBuilder, AddTorrent, AddTorrentOptions, AddTorrentResponse,
    ManagedTorrent, PeerConnectionOptions, Session, SessionOptions,
};
use librqbit_core::Id20;
use serde_bencode;
use serde_bytes::ByteBuf;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tracing::Span;

struct CdnClient {
    db: SqlitePool,
    session: Arc<Session>,
    out_dir: PathBuf,
}

fn generate_torrent(path: &str, piece_langth: i64) -> TorrentMetaV1<ByteBufOwned> {
    let torrent = TorrentBuilder::new(path, piece_langth).build().unwrap();
    torrent.write_into_file("sample.torrent").unwrap();
    todo!("generate torrent from chunks")
}

async fn download_content(_url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    todo!()
}

impl CdnClient {
    async fn new(out_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Connect to the SQLite database
        let db = SqlitePool::connect("sqlite:cdn_cache.db").await?;

        // Create the 'torrents' table if it doesn't exist
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS torrents (
                url TEXT PRIMARY KEY,
                torrent_file BLOB,
                created_at INTEGER,
                expires_at INTEGER
            )",
        )
        .execute(&db)
        .await?;

        // Configure session options
        let sopts = SessionOptions {
            disable_dht: false,
            disable_dht_persistence: false,
            dht_config: None,
            persistence: false,
            persistence_filename: None,
            peer_id: None,
            peer_opts: Some(PeerConnectionOptions {
                connect_timeout: Some(Duration::from_secs(2)),
                read_write_timeout: Some(Duration::from_secs(10)),
                ..Default::default()
            }),
            listen_port_range: None,
            enable_upnp_port_forwarding: true,
        };

        // Create a new session with the specified options
        let session = Session::new_with_opts(PathBuf::from("cdn_cache"), sopts)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self {
            db,
            session,
            out_dir,
        })
    }

    async fn fetch_content(
        &self,
        url: &str,
        chunk_size: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if a torrent exists for the given URL and if it hasn't expired
        let torrent_row =
            sqlx::query("SELECT torrent_file, expires_at FROM torrents WHERE url = ?")
                .bind(url)
                .fetch_optional(&self.db)
                .await?;

        if let Some(row) = torrent_row {
            let torrent_file: Vec<u8> = row.get(0);
            let expires_at: i64 = row.get(1);

            if expires_at > chrono::Utc::now().timestamp() {
                // Torrent exists and hasn't expired, use BitTorrent to fetch the content
                let torrent: librqbit::TorrentMetaV1<ByteBufOwned> =
                    torrent_from_bytes(&torrent_file)?;
                let torrent_id =
                    Id20::from_str("00000fffffffffffffffffffffffffffffffffff").unwrap(); // Replace with actual torrent ID

                // Create a new ManagedTorrentBuilder with the torrent info and output directory
                let mut builder =
                    ManagedTorrentBuilder::new(torrent.info, torrent_id, &self.out_dir);
                builder.overwrite(true);

                // Build the ManagedTorrent instance
                let torrent = builder.build(Span::current())?;

                // Fetch the content using BitTorrent
                self.fetch_content_from_torrent(torrent).await?;
                return Ok(());
            }
        }

        s3_client::download_stream_public_url(
            "https://bacbone-assets.s3.us-west-2.amazonaws.com/file_example_MP4_480_1_5MG.mp4",
            "file_output.mp4",
        )
        .await
        .unwrap();

        // Generate the torrent file from the content chunks
        let torrent = generate_torrent("file_output.mp4", 1_500_000);
        let torrent_file = serde_bencode::to_bytes(&torrent)?;

        // Save the torrent to the database
        sqlx::query(
            "INSERT OR REPLACE INTO torrents (url, torrent_file, created_at, expires_at)
            VALUES (?, ?, ?, ?)",
        )
        .bind(url)
        .bind(&torrent_file)
        .bind(chrono::Utc::now().timestamp())
        .bind(chrono::Utc::now().timestamp() + 3600) // Expires in 1 hour
        .execute(&self.db)
        .await?;

        // Fetch the content using BitTorrent
        let torrent_id = Id20::from_str("00000fffffffffffffffffffffffffffffffffff").unwrap(); // Replace with actual torrent ID
        let mut builder = ManagedTorrentBuilder::new(torrent.info, torrent_id, &self.out_dir);
        builder.overwrite(true);
        let torrent = builder.build(Span::current())?;

        self.fetch_content_from_torrent(torrent).await?;

        Ok(())
    }

    async fn fetch_content_from_torrent(
        &self,
        torrent: Arc<ManagedTorrent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Add the torrent to the session
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(serde_bencode::to_bytes(&torrent.info().info)?),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;

        match handle {
            AddTorrentResponse::Added(_, handle) => {
                // Wait until the torrent download is completed
                handle.wait_until_completed().await?;
                Ok(())
            }
            _ => Err("torrent already added or invalid".into()),
        }
    }

    async fn start_tracker_sync(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Expose a port for the tracker to send updates
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

        loop {
            // Accept incoming connections from the tracker
            let (mut socket, _) = listener.accept().await?;
            let db = self.db.clone();

            tokio::spawn(async move {
                // Read the torrent file from the socket
                let mut torrent_file = ByteBuf::new();
                if let Err(e) = socket.read_to_end(&mut torrent_file).await {
                    eprintln!("Error reading torrent file from socket: {:?}", e);
                    return;
                }

                let torrent: librqbit::TorrentMetaV1<ByteBuf> = torrent_from_bytes(&torrent_file)
                    .map_err(|e| format!("Error parsing torrent file: {:?}", e))
                    .unwrap();

                // Save the torrent to the database
                if let Err(e) = sqlx::query(
                    "INSERT OR REPLACE INTO torrents (url, torrent_file, created_at, expires_at)
                    VALUES (?, ?, ?, ?)",
                )
                .bind(torrent.info_hash.as_string())
                .bind(torrent_file.to_vec())
                .bind(chrono::Utc::now().timestamp())
                .bind(chrono::Utc::now().timestamp() + 3600) // Expires in 1 hour
                .execute(&db)
                .await
                {
                    eprintln!("Error saving torrent to database: {:?}", e);
                    return;
                }
            });
        }
    }
}
