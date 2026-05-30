use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};
use uuid::Uuid;
use wallflow_common::RendererId;
use wallflow_media::{platform_video_backend, NullVideoBackend, VideoBackend};

#[derive(Debug, Parser)]
#[command(name = "wallflow-renderer")]
#[command(about = "WallFlow isolated renderer process")]
struct Args {
    #[arg(long)]
    renderer_id: Option<Uuid>,

    #[arg(long, default_value = "primary")]
    monitor: String,

    #[arg(long, default_value = "none")]
    wallpaper: String,

    #[arg(long)]
    source: Option<PathBuf>,

    #[arg(long, default_value_t = true)]
    muted: bool,

    #[arg(long, default_value_t = true)]
    looping: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();
    let renderer_id = RendererId(args.renderer_id.unwrap_or_else(Uuid::new_v4));

    info!(?renderer_id, monitor = %args.monitor, wallpaper = %args.wallpaper, "renderer starting");

    match args.wallpaper.as_str() {
        "none" => {
            info!("no wallpaper requested; renderer stays alive for smoke testing");
        }
        "video" => {
            let Some(source) = args.source.as_deref() else {
                warn!("video wallpaper requested without --source");
                return Ok(());
            };

            let mut backend: Box<dyn VideoBackend> = match platform_video_backend() {
                Ok(backend) => backend,
                Err(err) => {
                    warn!(error = %err, "platform backend unavailable; falling back to null backend");
                    Box::new(NullVideoBackend::default())
                }
            };
            backend.load(source)?;
            backend.set_looping(args.looping)?;
            backend.set_volume(if args.muted { 0.0 } else { 1.0 })?;
            backend.play()?;
            info!(source = %source.display(), "video backend started");
        }
        "static" => {
            let Some(source) = args.source.as_deref() else {
                warn!("static wallpaper requested without --source");
                return Ok(());
            };
            info!(source = %source.display(), "static renderer path is reserved for the next winit/wgpu pass");
        }
        other => {
            warn!(wallpaper = other, "unsupported wallpaper kind");
        }
    }

    // MVP smoke behavior: keep process alive until manually stopped.
    // The next agent task should replace this with the winit event loop and IPC reader.
    std::thread::park();
    Ok(())
}
