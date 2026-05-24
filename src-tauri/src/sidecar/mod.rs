use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// why: first `uv run` on a clean machine sets up the venv and resolves deps,
// which routinely takes 10-20s. 5s would fire before python ever starts.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex};

type SideErr = Box<dyn Error + Send + Sync>;
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Serialize)]
struct Request<'a> {
    id: u64,
    op: &'a str,
    payload: Value,
}

#[derive(Deserialize)]
struct Response {
    id: u64,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

pub struct Sidecar {
    _child: Child,
    stdin: Mutex<ChildStdin>,
    pending: PendingMap,
    next_id: AtomicU64,
}

impl Sidecar {
    pub async fn spawn_dev() -> Result<Self, SideErr> {
        let python_dir = python_dir()?;
        let mut child = Command::new("uv")
            .args(["run", "python", "main.py"])
            .current_dir(&python_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn `uv run python main.py` in {python_dir:?}: {e}"))?;

        let stdin = child.stdin.take().ok_or("missing child stdin")?;
        let stdout = child.stdout.take().ok_or("missing child stdout")?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let reader_pending = pending.clone();

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => match serde_json::from_str::<Response>(&line) {
                        Ok(resp) => {
                            let sender = reader_pending.lock().await.remove(&resp.id);
                            if let Some(tx) = sender {
                                let payload = match (resp.result, resp.error) {
                                    (Some(v), _) => Ok(v),
                                    (None, Some(e)) => Err(e),
                                    (None, None) => Err("empty response".into()),
                                };
                                let _ = tx.send(payload);
                            }
                        }
                        Err(e) => eprintln!("sidecar: bad line {line:?}: {e}"),
                    },
                    Ok(None) => {
                        eprintln!("sidecar: stdout closed");
                        break;
                    }
                    Err(e) => {
                        eprintln!("sidecar: read error: {e}");
                        break;
                    }
                }
            }
            let mut p = reader_pending.lock().await;
            for (_, tx) in p.drain() {
                let _ = tx.send(Err("sidecar stdout closed".into()));
            }
        });

        Ok(Self {
            _child: child,
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicU64::new(1),
        })
    }

    /// Dispatch one op and await its response.
    ///
    /// The two-level result separates a transport/infra failure from a per-op
    /// application error so the caller can react differently:
    /// - `Ok(Ok(v))`  — the op succeeded with value `v`.
    /// - `Ok(Err(e))` — the op RAN but Python returned `{id, error}` (a per-file
    ///   application error, e.g. a corrupt image). The sidecar is still healthy.
    /// - `Err(e)`     — transport/infra failure (serialize, write, reader dropped,
    ///   timeout). The sidecar may be down; the request did not complete.
    pub async fn call(&self, op: &str, payload: Value) -> Result<Result<Value, String>, SideErr> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = Request { id, op, payload };
        let line = serde_json::to_string(&req)?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        // why: on write failure the pending entry would leak; drop it before propagating
        if let Err(e) = self.write_line(&line).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }

        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            // op ran and succeeded
            Ok(Ok(Ok(v))) => Ok(Ok(v)),
            // op ran but returned an application error — per-file, not transport
            Ok(Ok(Err(e))) => Ok(Err(e)),
            // oneshot sender dropped (reader loop exited) — transport failure
            Ok(Err(_)) => Err("sidecar reader dropped".into()),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(format!("sidecar timeout ({}s)", CALL_TIMEOUT.as_secs()).into())
            }
        }
    }

    async fn write_line(&self, line: &str) -> Result<(), SideErr> {
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }
}

fn python_dir() -> Result<PathBuf, SideErr> {
    // why: CARGO_MANIFEST_DIR is a build-machine path. spawn_dev only works
    // when running from the source tree (cargo tauri dev). Release bundles
    // must ship a sidecar binary (planned for M1+).
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let dir = PathBuf::from(manifest_dir).join("..").join("python");
    dir.canonicalize().map_err(|e| {
        format!("spawn_dev expects {dir:?} to exist (dev only; bundled sidecar lands in M1+): {e}")
            .into()
    })
}
