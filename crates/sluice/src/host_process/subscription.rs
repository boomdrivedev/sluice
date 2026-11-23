//! A subscription: one connection that serves many folders (the change
//! `sluice-host-answers-every-read`, design D6, D10;
//! `one-subscription-per-plugin@dcc7b7fec9ee`).
//!
//! The client sends `{"add":"<folder>"}` and `{"remove":"<folder>"}` lines.
//! An `add` line can name how the client follows the folder, as `as`. The
//! resident sends a `document` line for a folder when the folder is added,
//! and after that each time the folder's document changes. A folder that
//! the client follows so that it gets reports gets a `pass` line for each
//! report, in order.
//!
//! The connection's task holds a `StreamMap` of `WatchStream`s of the
//! documents and a `StreamMap` of the reports, each keyed by folder, and a
//! guard for each folder. It waits in one `select!` on the client's next
//! line, the next document, the next report and its token. While the task
//! waits for a write, it does not poll the documents. A `watch` channel
//! holds one value, so a slow client gets only the newest document of each
//! folder.
//!
//! This module names no type of sluice. It uses only tokio, tokio-util,
//! tokio-stream, futures-util and serde.

use std::future::Future;
use std::path::PathBuf;

use futures_util::SinkExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::{UnboundedReceiverStream, WatchStream};
use tokio_stream::{StreamExt, StreamMap};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};
use tokio_util::sync::CancellationToken;

/// A line that the client sends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Ask<K> {
    /// Serve `add`, followed as `as` says, or as the default.
    Add {
        add: PathBuf,
        #[serde(rename = "as", default, skip_serializing_if = "Option::is_none")]
        kind: Option<K>,
    },
    /// Serve `remove` no more.
    Remove { remove: PathBuf },
}

/// A folder's document, as the resident sends it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document<D> {
    pub folder: PathBuf,
    pub overview: D,
}

/// A folder's report, as the resident sends it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pass<R> {
    pub folder: PathBuf,
    pub report: R,
}

/// A line that the resident sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Sent<D, R> {
    Document(Document<D>),
    Pass(Pass<R>),
    /// A line that the resident could not read, in these words.
    Error(String),
}

/// What a folder is served with.
pub struct Served<D, R, G> {
    /// The folder's documents. `None` is a document not yet made, which
    /// the loop passes over.
    pub documents: watch::Receiver<Option<D>>,
    /// The folder's reports, where it is followed so.
    pub reports: Option<mpsc::UnboundedReceiver<R>>,
    /// What the loop drops when the folder is removed or the connection
    /// ends.
    pub guard: G,
}

/// What a resident serves on a subscription.
pub trait Serves: Send + Sync {
    /// How a client follows a folder.
    type Kind: DeserializeOwned + Default + Send;
    /// A folder's document.
    type Document: Clone + Serialize + Send + Sync + 'static;
    /// A folder's report.
    type Report: Serialize + Send + 'static;
    /// What keeps a folder followed while the loop holds it.
    type Guard: Send;

    /// Serve `folder`, followed as `kind`. Where it cannot be served, the
    /// document that says why, which the loop sends once.
    fn serve(
        &self,
        folder: PathBuf,
        kind: Self::Kind,
    ) -> impl Future<
        Output = Result<Served<Self::Document, Self::Report, Self::Guard>, Self::Document>,
    > + Send;

    /// Whether `document` ends its folder's part of the subscription: its
    /// repository is gone, or has no sluice.
    fn ends(&self, document: &Self::Document) -> bool;
}

/// What a connection's loop holds.
struct Held<S: Serves> {
    documents: StreamMap<PathBuf, WatchStream<Option<S::Document>>>,
    passes: StreamMap<PathBuf, UnboundedReceiverStream<S::Report>>,
    guards: std::collections::BTreeMap<PathBuf, S::Guard>,
}

impl<S: Serves> Held<S> {
    fn remove(&mut self, folder: &PathBuf) {
        self.documents.remove(folder);
        self.passes.remove(folder);
        self.guards.remove(folder);
    }
}

/// Serve a subscription on `lines` and `sink`, `folders` added first,
/// until the client closes it, sends a line that cannot be read, or
/// `token` is cancelled.
pub async fn serve<S, R, W>(
    serves: &S,
    folders: Vec<(PathBuf, S::Kind)>,
    mut lines: FramedRead<R, LinesCodec>,
    mut sink: FramedWrite<W, LinesCodec>,
    token: CancellationToken,
) -> Result<(), String>
where
    S: Serves,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut held: Held<S> = Held {
        documents: StreamMap::new(),
        passes: StreamMap::new(),
        guards: Default::default(),
    };
    for (folder, kind) in folders {
        if let Some(sent) = add(serves, &mut held, folder, kind).await {
            send::<S, _>(&mut sink, &sent).await?;
        }
    }
    loop {
        tokio::select! {
            () = token.cancelled() => return Ok(()),
            line = lines.next() => {
                let Some(Ok(line)) = line else {
                    return Ok(());
                };
                match serde_json::from_str::<Ask<S::Kind>>(line.trim_end()) {
                    Ok(Ask::Add { add: folder, kind }) => {
                        let kind = kind.unwrap_or_default();
                        if let Some(sent) = add(serves, &mut held, folder, kind).await {
                            send::<S, _>(&mut sink, &sent).await?;
                        }
                    }
                    Ok(Ask::Remove { remove }) => held.remove(&remove),
                    Err(e) => {
                        let words = format!("a subscription's line that could not be read: {e}");
                        let _ = send::<S, _>(&mut sink, &Sent::Error(words.clone())).await;
                        return Err(words);
                    }
                }
            }
            Some((folder, document)) = held.documents.next(), if !held.documents.is_empty() => {
                // A document not yet made: the first reading's follows.
                let Some(document) = document else {
                    continue;
                };
                let ends = serves.ends(&document);
                let sent = Sent::Document(Document { folder: folder.clone(), overview: document });
                send::<S, _>(&mut sink, &sent).await?;
                if ends {
                    held.remove(&folder);
                }
            }
            Some((folder, report)) = held.passes.next(), if !held.passes.is_empty() => {
                send::<S, _>(&mut sink, &Sent::Pass(Pass { folder, report })).await?;
            }
        }
    }
}

/// Add `folder`, followed as `kind`, replacing what the loop held for it;
/// where it cannot be served, the document to send for it.
async fn add<S: Serves>(
    serves: &S,
    held: &mut Held<S>,
    folder: PathBuf,
    kind: S::Kind,
) -> Option<Sent<S::Document, S::Report>> {
    held.remove(&folder);
    match serves.serve(folder.clone(), kind).await {
        Ok(served) => {
            held.documents
                .insert(folder.clone(), WatchStream::new(served.documents));
            if let Some(reports) = served.reports {
                held.passes
                    .insert(folder.clone(), UnboundedReceiverStream::new(reports));
            }
            held.guards.insert(folder, served.guard);
            None
        }
        Err(document) => Some(Sent::Document(Document {
            folder,
            overview: document,
        })),
    }
}

/// Send `sent` as one line.
async fn send<S: Serves, W: AsyncWrite + Unpin>(
    sink: &mut FramedWrite<W, LinesCodec>,
    sent: &Sent<S::Document, S::Report>,
) -> Result<(), String> {
    let line = serde_json::to_string(sent).map_err(|e| e.to_string())?;
    sink.send(line).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[derive(Default, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Kind {
        #[default]
        Watch,
        Run,
    }

    /// A guard that counts the guards alive.
    struct Counted(Arc<AtomicUsize>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// Folders and their channels, as a test holds them.
    #[derive(Default)]
    struct Fake {
        documents: Mutex<BTreeMap<PathBuf, watch::Sender<Option<String>>>>,
        reports: Mutex<BTreeMap<PathBuf, mpsc::UnboundedSender<String>>>,
        alive: Arc<AtomicUsize>,
        served: AtomicUsize,
    }

    impl Fake {
        fn document(&self, folder: &str, document: &str) {
            let mut documents = self.documents.lock().unwrap();
            let tx = documents
                .entry(PathBuf::from(folder))
                .or_insert_with(|| watch::Sender::new(None));
            tx.send_replace(Some(document.to_string()));
        }

        fn report(&self, folder: &str, report: &str) -> bool {
            self.reports
                .lock()
                .unwrap()
                .get(&PathBuf::from(folder))
                .is_some_and(|tx| tx.send(report.to_string()).is_ok())
        }
    }

    impl Serves for Fake {
        type Kind = Kind;
        type Document = String;
        type Report = String;
        type Guard = Counted;

        async fn serve(
            &self,
            folder: PathBuf,
            kind: Kind,
        ) -> Result<Served<String, String, Counted>, String> {
            if folder.as_path() == std::path::Path::new("/gone") {
                return Err("gone".into());
            }
            self.served.fetch_add(1, Ordering::SeqCst);
            let documents = self
                .documents
                .lock()
                .unwrap()
                .entry(folder.clone())
                .or_insert_with(|| watch::Sender::new(None))
                .subscribe();
            let reports = match kind {
                Kind::Run => {
                    let (tx, rx) = mpsc::unbounded_channel();
                    self.reports.lock().unwrap().insert(folder, tx);
                    Some(rx)
                }
                Kind::Watch => None,
            };
            self.alive.fetch_add(1, Ordering::SeqCst);
            Ok(Served {
                documents,
                reports,
                guard: Counted(self.alive.clone()),
            })
        }

        fn ends(&self, document: &String) -> bool {
            document == "gone"
        }
    }

    /// A client's end of a subscription that `fake` serves, its loop on a
    /// task that ends with `token`.
    struct Client {
        lines: tokio::io::Lines<BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
        write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
        ended: tokio::task::JoinHandle<Result<(), String>>,
    }

    impl Client {
        fn start(fake: Arc<Fake>, token: CancellationToken) -> Client {
            let (client, server) = tokio::io::duplex(1 << 16);
            let (sr, sw) = tokio::io::split(server);
            let ended = tokio::spawn(async move {
                serve(
                    &*fake,
                    Vec::new(),
                    FramedRead::new(sr, LinesCodec::new()),
                    FramedWrite::new(sw, LinesCodec::new()),
                    token,
                )
                .await
            });
            let (cr, write) = tokio::io::split(client);
            Client {
                lines: BufReader::new(cr).lines(),
                write,
                ended,
            }
        }

        async fn say(&mut self, line: &str) {
            self.write
                .write_all(format!("{line}\n").as_bytes())
                .await
                .unwrap();
        }

        /// The next line, within a second; `None` where none came.
        async fn next(&mut self) -> Option<serde_json::Value> {
            let line = tokio::time::timeout(Duration::from_secs(1), self.lines.next_line())
                .await
                .ok()?
                .ok()??;
            Some(serde_json::from_str(&line).unwrap())
        }
    }

    #[tokio::test]
    async fn an_added_folder_gets_its_document_at_once() {
        let fake = Arc::new(Fake::default());
        fake.document("/a", "a1");
        let mut c = Client::start(fake.clone(), CancellationToken::new());
        c.say(r#"{"add":"/a"}"#).await;
        assert_eq!(
            c.next().await.unwrap(),
            serde_json::json!({"document": {"folder": "/a", "overview": "a1"}})
        );
        // A folder with no document yet gets its first one when it comes.
        c.say(r#"{"add":"/b"}"#).await;
        assert_eq!(c.next().await, None);
        fake.document("/b", "b1");
        assert_eq!(
            c.next().await.unwrap()["document"]["overview"],
            serde_json::json!("b1")
        );
    }

    #[tokio::test]
    async fn a_removed_folder_leaves_both_maps() {
        let fake = Arc::new(Fake::default());
        fake.document("/a", "a1");
        let mut c = Client::start(fake.clone(), CancellationToken::new());
        c.say(r#"{"add":"/a","as":"run"}"#).await;
        assert!(c.next().await.is_some());
        assert!(fake.report("/a", "pass 1"));
        assert_eq!(
            c.next().await.unwrap(),
            serde_json::json!({"pass": {"folder": "/a", "report": "pass 1"}})
        );
        c.say(r#"{"remove":"/a"}"#).await;
        // Its guard is dropped, and its streams with it.
        tokio::time::timeout(Duration::from_secs(1), async {
            while fake.alive.load(Ordering::SeqCst) != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!fake.report("/a", "pass 2"), "no stream holds its reports");
        fake.document("/a", "a2");
        assert_eq!(c.next().await, None);
    }

    #[tokio::test]
    async fn an_add_of_a_held_folder_replaces_its_stream() {
        let fake = Arc::new(Fake::default());
        fake.document("/a", "a1");
        let mut c = Client::start(fake.clone(), CancellationToken::new());
        c.say(r#"{"add":"/a"}"#).await;
        assert!(c.next().await.is_some());
        c.say(r#"{"add":"/a"}"#).await;
        // The new stream gives the current document at once.
        assert_eq!(
            c.next().await.unwrap()["document"]["overview"],
            serde_json::json!("a1")
        );
        assert_eq!(fake.served.load(Ordering::SeqCst), 2);
        assert_eq!(fake.alive.load(Ordering::SeqCst), 1, "one guard is held");
        fake.document("/a", "a2");
        assert_eq!(
            c.next().await.unwrap()["document"]["overview"],
            serde_json::json!("a2")
        );
        assert_eq!(c.next().await, None, "one stream, one line");
    }

    #[tokio::test]
    async fn a_cancelled_token_ends_the_connection() {
        let fake = Arc::new(Fake::default());
        let token = CancellationToken::new();
        let mut c = Client::start(fake.clone(), token.clone());
        c.say(r#"{"add":"/a"}"#).await;
        token.cancel();
        let ended = tokio::time::timeout(Duration::from_secs(1), &mut c.ended)
            .await
            .unwrap();
        assert_eq!(ended.unwrap(), Ok(()));
        assert_eq!(fake.alive.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_closed_client_drops_each_follower() {
        let fake = Arc::new(Fake::default());
        fake.document("/a", "a1");
        fake.document("/b", "b1");
        let mut c = Client::start(fake.clone(), CancellationToken::new());
        c.say(r#"{"add":"/a"}"#).await;
        c.say(r#"{"add":"/b","as":"run"}"#).await;
        assert!(c.next().await.is_some());
        assert!(c.next().await.is_some());
        assert_eq!(fake.alive.load(Ordering::SeqCst), 2);
        c.write.shutdown().await.unwrap();
        let ended = tokio::time::timeout(Duration::from_secs(1), &mut c.ended)
            .await
            .unwrap();
        assert_eq!(ended.unwrap(), Ok(()));
        assert_eq!(fake.alive.load(Ordering::SeqCst), 0);
    }

    /// A folder whose document ends it leaves the subscription, and the
    /// other folders stay served.
    #[tokio::test]
    async fn a_folder_that_ends_leaves_and_the_others_stay() {
        let fake = Arc::new(Fake::default());
        fake.document("/a", "a1");
        fake.document("/b", "b1");
        let mut c = Client::start(fake.clone(), CancellationToken::new());
        c.say(r#"{"add":"/a"}"#).await;
        c.say(r#"{"add":"/b"}"#).await;
        assert!(c.next().await.is_some());
        assert!(c.next().await.is_some());
        fake.document("/a", "gone");
        assert_eq!(
            c.next().await.unwrap()["document"]["overview"],
            serde_json::json!("gone")
        );
        fake.document("/a", "a3");
        fake.document("/b", "b2");
        assert_eq!(
            c.next().await.unwrap(),
            serde_json::json!({"document": {"folder": "/b", "overview": "b2"}})
        );
        // A folder that cannot be served gets its document and no stream.
        c.say(r#"{"add":"/gone"}"#).await;
        assert_eq!(
            c.next().await.unwrap()["document"]["overview"],
            serde_json::json!("gone")
        );
        assert_eq!(fake.alive.load(Ordering::SeqCst), 1);
    }
}
