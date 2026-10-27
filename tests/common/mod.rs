use remarkable_cloud::{
    Client, ClientOptions, Config, Endpoints, FileUpdate, RootState, SchemaEntry, content_hash,
    encode_cloud_manifest,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Response, Server};
pub const SOURCE: &str = "11111111-1111-4111-8111-111111111111";
pub const DEST: &str = "22222222-2222-4222-8222-222222222222";
pub const FOLDER: &str = "33333333-3333-4333-8333-333333333333";
pub const PDF: &[u8] = include_bytes!("../fixtures/links.pdf");
pub const RM: &[u8] = include_bytes!("../fixtures/sample.rm");
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}
pub struct State {
    pub root: RootState,
    pub blobs: HashMap<String, Vec<u8>>,
    pub requests: Vec<Request>,
    pub commit_status: u16,
    pub root_delay: Duration,
    pub reject_status: u16,
    pub corrupt_after_commit: bool,
    pub guard_change: bool,
    pub root_reads: usize,
}
impl State {
    pub fn document(&mut self, id: &str, name: &str, parent: &str, content: Value, ink: bool) {
        let mut files=vec![FileUpdate{name:format!("{id}.metadata"),data:serde_json::to_vec(&json!({"type":"DocumentType","visibleName":name,"parent":parent,"lastModified":"1000","unknown":9007199254740993u64})).unwrap()},FileUpdate{name:format!("{id}.content"),data:serde_json::to_vec(&content).unwrap()},FileUpdate{name:format!("{id}.pdf"),data:PDF.into()},FileUpdate{name:format!("{id}.pagedata"),data:b"Blank\nBlank\n".to_vec()}];
        if ink {
            files.push(FileUpdate {
                name: format!("{id}/p0.rm"),
                data: RM.into(),
            });
        }
        self.put_item(id, files);
    }
    pub fn collection(&mut self, id: &str, name: &str, parent: &str) {
        self.put_item(
            id,
            vec![FileUpdate {
                name: format!("{id}.metadata"),
                data: serde_json::to_vec(
                    &json!({"type":"CollectionType","visibleName":name,"parent":parent}),
                )
                .unwrap(),
            }],
        );
    }
    pub fn put_item(&mut self, id: &str, files: Vec<FileUpdate>) {
        let mut entries = Vec::new();
        for file in &files {
            let hash = content_hash(&file.data);
            self.blobs.insert(hash.clone(), file.data.clone());
            entries.push(SchemaEntry::new(
                &hash,
                &file.name,
                0,
                file.data.len() as i64,
            ));
        }
        let document = encode_cloud_manifest(&entries, false).unwrap();
        self.blobs.insert(document.hash.clone(), document.data);
        let old = remarkable_cloud::parse_manifest(
            std::str::from_utf8(&self.blobs[&self.root.hash]).unwrap(),
            &self.root.hash,
        )
        .unwrap();
        let mut entries: Vec<_> = old
            .entries
            .into_iter()
            .filter(|e| e.id != "." && e.id != id)
            .collect();
        entries.push(SchemaEntry::new(
            &document.hash,
            id,
            files.len() as i64,
            document.size,
        ));
        let root = encode_cloud_manifest(&entries, true).unwrap();
        self.root.hash = root.hash.clone();
        self.blobs.insert(root.hash, root.data);
    }
    pub fn files(&self, id: &str) -> BTreeMap<String, Vec<u8>> {
        let root = remarkable_cloud::parse_manifest(
            std::str::from_utf8(&self.blobs[&self.root.hash]).unwrap(),
            &self.root.hash,
        )
        .unwrap();
        let hash = &root.find(id).unwrap().hash;
        let doc =
            remarkable_cloud::parse_manifest(std::str::from_utf8(&self.blobs[hash]).unwrap(), hash)
                .unwrap();
        doc.entries
            .into_iter()
            .map(|e| (e.id, self.blobs[&e.hash].clone()))
            .collect()
    }
    pub fn commits(&self) -> usize {
        self.requests
            .iter()
            .filter(|r| r.method == "PUT" && r.path == "/sync/v3/root")
            .count()
    }
    pub fn writes(&self) -> usize {
        self.requests.iter().filter(|r| r.method == "PUT").count()
    }
}
pub fn content(ids: &[&str]) -> Value {
    json!({"fileType":"pdf","pageCount":ids.len(),"pages":ids,"zoomMode":"bestFit","unknown":{"large":9007199254740993u64}})
}
pub struct Cloud {
    pub url: String,
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Default for Cloud {
    fn default() -> Self {
        Self::new()
    }
}
impl Cloud {
    pub fn new() -> Self {
        let empty = encode_cloud_manifest(&[], true).unwrap();
        let state = Arc::new(Mutex::new(State {
            root: RootState {
                hash: empty.hash.clone(),
                generation: 4,
                schema_version: 4,
            },
            blobs: HashMap::from([(empty.hash, empty.data)]),
            requests: vec![],
            commit_status: 200,
            root_delay: Duration::ZERO,
            reject_status: 0,
            corrupt_after_commit: false,
            guard_change: false,
            root_reads: 0,
        }));
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let stopped = stop.clone();
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Some(mut incoming) = server.recv_timeout(Duration::from_millis(10)).unwrap()
                else {
                    continue;
                };
                let mut body = Vec::new();
                incoming.as_reader().read_to_end(&mut body).unwrap();
                let request = Request {
                    method: incoming.method().to_string(),
                    path: incoming.url().into(),
                    headers: incoming
                        .headers()
                        .iter()
                        .map(|h| {
                            (
                                h.field.to_string().to_ascii_lowercase(),
                                h.value.to_string(),
                            )
                        })
                        .collect(),
                    body,
                };
                let (status, body, delay) = {
                    let mut state = shared.lock().unwrap();
                    state.requests.push(request.clone());
                    if state.reject_status != 0 {
                        (state.reject_status, b"rejected".to_vec(), Duration::ZERO)
                    } else if request.path == "/token/json/2/device/new" {
                        (200, b"fake-device".to_vec(), Duration::ZERO)
                    } else if request.path == "/token/json/2/user/new" {
                        (200, b"fake-user".to_vec(), Duration::ZERO)
                    } else if request.path == "/sync/v3/root" {
                        if request.method == "PUT" {
                            if state.commit_status == 200 {
                                let root: Value = serde_json::from_slice(&request.body).unwrap();
                                assert_eq!(root["generation"], state.root.generation);
                                state.root.hash = root["hash"].as_str().unwrap().into();
                                state.root.generation += 1;
                                if state.corrupt_after_commit {
                                    let hash = state.root.hash.clone();
                                    state.blobs.insert(hash, b"invalid".to_vec());
                                }
                            }
                            (
                                state.commit_status,
                                serde_json::to_vec(&state.root).unwrap(),
                                state.root_delay,
                            )
                        } else {
                            state.root_reads += 1;
                            if state.guard_change && state.root_reads > 2 {
                                state.root.generation += 1;
                            }
                            (
                                200,
                                serde_json::to_vec(&state.root).unwrap(),
                                Duration::ZERO,
                            )
                        }
                    } else if let Some(hash) = request.path.strip_prefix("/sync/v3/files/") {
                        if request.method == "PUT" {
                            assert!(request.headers.contains_key("rm-filename"));
                            state.blobs.insert(hash.into(), request.body);
                            (201, vec![], Duration::ZERO)
                        } else {
                            state
                                .blobs
                                .get(hash)
                                .map_or((404, b"missing".to_vec(), Duration::ZERO), |b| {
                                    (200, b.clone(), Duration::ZERO)
                                })
                        }
                    } else {
                        (404, b"unknown endpoint".to_vec(), Duration::ZERO)
                    }
                };
                thread::sleep(delay);
                let response = Response::from_data(body)
                    .with_status_code(status)
                    .with_header(
                        Header::from_bytes("Content-Type", "application/octet-stream").unwrap(),
                    );
                let _ = incoming.respond(response);
            }
        });
        Self {
            url,
            state,
            stop,
            thread: Some(thread),
        }
    }
    pub fn seeded() -> Self {
        let cloud = Self::new();
        {
            let mut state = cloud.state.lock().unwrap();
            state.collection(FOLDER, "Folder", "");
            state.document(SOURCE, "Source", "", content(&["p0", "p1"]), true);
            state.document(DEST, "Destination", FOLDER, content(&["d0", "d1"]), false);
        }
        cloud
    }
    pub fn client(&self) -> Client {
        Client::new(ClientOptions {
            config: Config {
                device_token: "fake-device".into(),
                user_token: "fake-user".into(),
            },
            endpoints: Endpoints {
                raw_host: self.url.clone(),
                storage_host: self.url.clone(),
                webapp_host: self.url.clone(),
            },
            auth_base_url: self.url.clone(),
            auto_renew: false,
            ..Default::default()
        })
        .unwrap()
    }
    pub fn config(&self, path: &std::path::Path) {
        Config {
            device_token: "fake-device".into(),
            user_token: "fake-user".into(),
        }
        .write_file(path)
        .unwrap();
    }
}
impl Drop for Cloud {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
