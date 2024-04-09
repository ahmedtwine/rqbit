use anyhow::Context;
use librqbit::{
    torrent_state::ManagedTorrentBuilder, AddTorrent, AddTorrentOptions, AddTorrentResponse,
    ManagedTorrent, PeerConnectionOptions, Session, SessionOptions,
};
use librqbit_core::torrent_metainfo::{TorrentMetaV1File, TorrentMetaV1Info, TorrentMetaV1Owned};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;

struct CdnClient {
    db: SqlitePool,
    session: Arc<Session>,
}

fn generate_torrent(chunks: &[Vec<u8>]) -> TorrentMetaV1Info<Vec<u8>> {
    todo!("generate torrent from chunks")
}

async fn download_content(url: &str) -> () {
    todo!()
}

impl CdnClient {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let db = SqlitePool::connect("sqlite:cdn_cache.db").await?;
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

        let session = Session::new_with_opts(PathBuf::from("cdn_cache"), sopts)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self { db, session })
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
                let torrent_info =
                    TorrentMetaV1Info::from_bytes(torrent_file).context("invalid torrent file")?;
                let torrent = ManagedTorrentBuilder::new(torrent_info, self.session.clone())
                    .build()
                    .context("failed to create managed torrent")?;
                self.fetch_content_from_torrent(torrent).await?;
                return Ok(());
            }
        }

        // Torrent doesn't exist or has expired, fetch from the upstream CDN
        let content = download_content(url).await?;

        // Chunk the content
        let chunks = content
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();

        // Generate the torrent file
        let torrent = generate_torrent(&chunks);
        let torrent_file = torrent.to_bytes();

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
        let torrent_info =
            TorrentMetaV1Info::from_bytes(torrent_file).context("invalid torrent file")?;
        let torrent = ManagedTorrentBuilder::new(torrent_info, self.session.clone())
            .build()
            .context("failed to create managed torrent")?;
        self.fetch_content_from_torrent(torrent).await?;

        Ok(())
    }

    async fn fetch_content_from_torrent(
        &self,
        torrent: Arc<ManagedTorrent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_torrent(torrent.info().info.clone()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;

        match handle {
            AddTorrentResponse::Added(_, handle) => {
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
            let (mut socket, _) = listener.accept().await?;
            let db = self.db.clone();

            tokio::spawn(async move {
                // Read the torrent file from the socket
                let mut torrent_file = Vec::new();
                socket.read_to_end(&mut torrent_file).await?;

                // Parse the torrent file
                let torrent_info =
                    TorrentMetaV1Info::from_bytes(torrent_file).context("invalid torrent file")?;

                // Save the torrent to the database
                sqlx::query(
                    "INSERT OR REPLACE INTO torrents (url, torrent_file, created_at, expires_at)
                    VALUES (?, ?, ?, ?)",
                )
                .bind(torrent_info.name().to_string())
                .bind(&torrent_file)
                .bind(chrono::Utc::now().timestamp())
                .bind(chrono::Utc::now().timestamp() + 3600) // Expires in 1 hour
                .execute(&db)
                .await?;

                Ok(())
            });
        }
    }
}
