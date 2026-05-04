use std::collections::HashMap;
use std::path::PathBuf;

use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

pub(crate) enum WriteOp {
    Append { path: PathBuf, line: String },
}

pub(crate) const WRITE_QUEUE_CAPACITY: usize = 256;

pub(crate) fn spawn(mut rx: mpsc::Receiver<WriteOp>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut handles: HashMap<PathBuf, BufWriter<File>> = HashMap::new();
        while let Some(op) = rx.recv().await {
            match op {
                WriteOp::Append { path, line } => {
                    let writer = match get_or_open(&mut handles, &path).await {
                        Ok(w) => w,
                        Err(e) => {
                            warn!(target: "vantage_log_writer", path = %path.display(), error = %e, "open failed");
                            continue;
                        }
                    };
                    if let Err(e) = writer.write_all(line.as_bytes()).await {
                        warn!(target: "vantage_log_writer", path = %path.display(), error = %e, "write failed");
                        handles.remove(&path);
                        continue;
                    }
                    if let Err(e) = writer.flush().await {
                        warn!(target: "vantage_log_writer", path = %path.display(), error = %e, "flush failed");
                        handles.remove(&path);
                    }
                }
            }
        }
        for (path, mut writer) in handles {
            if let Err(e) = writer.flush().await {
                warn!(target: "vantage_log_writer", path = %path.display(), error = %e, "final flush failed");
            }
        }
    })
}

async fn get_or_open<'a>(
    handles: &'a mut HashMap<PathBuf, BufWriter<File>>,
    path: &PathBuf,
) -> std::io::Result<&'a mut BufWriter<File>> {
    if !handles.contains_key(path) {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        handles.insert(path.clone(), BufWriter::new(file));
    }
    Ok(handles.get_mut(path).unwrap())
}
