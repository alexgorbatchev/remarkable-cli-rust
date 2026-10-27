use crate::{
    error::{Error, Result, message},
    import, json,
    output::Output,
    snapshot, validation,
};
use remarkable_cloud::{
    Client, CreateDocumentOptions, CreateResult, ErrorKind, FileUpdate, ItemMetadata, RootState,
    UpdateState, content_hash, parse_manifest,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
pub const PENDING: &str = "pending-tablet-initialization";
pub const INITIALIZED: &str = "initialized";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UploadFile {
    pub name: String,
    pub sha256: String,
    pub size: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub version: i32,
    pub title: String,
    pub folder: String,
    pub pages: usize,
    pub native_pages: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub page_ids: Vec<String>,
    pub result: CreateResult,
    pub files: Vec<UploadFile>,
}
pub struct Options<'a> {
    pub title: &'a str,
    pub folder: &'a str,
    pub evidence: &'a Path,
    pub initialize_pages: bool,
}
pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
pub fn page_order(pages: usize) -> Result<Vec<String>> {
    if !(1..=816).contains(&pages) {
        return Err(message(format!(
            "--initialize-pages supports PDFs of 1 to 816 pages; this PDF has {pages}"
        )));
    }
    let mut order = Vec::new();
    for (marker, prefix, count) in [("", "", 325), ("nna", "nn", 312), ("nnnaa", "nnna", 177)] {
        if !marker.is_empty() {
            order.push(marker.to_string());
        }
        for i in 26..26 + count {
            order.push(format!(
                "{prefix}{}{}",
                char::from(b'a' + (i / 26) as u8),
                char::from(b'a' + (i % 26) as u8)
            ));
        }
    }
    order.truncate(pages);
    Ok(order)
}
pub fn files(id: &str, pdf: Vec<u8>, evidence: &Evidence, author: &str) -> Result<Vec<FileUpdate>> {
    validate_native_pages(evidence, id)?;
    let now = now_millis().to_string();
    let metadata = json!({"type":"DocumentType","visibleName":evidence.title,"parent":evidence.folder,"deleted":false,"pinned":false,"createdTime":now,"lastModified":now,"lastOpened":"0","lastOpenedPage":0});
    let mut content = json!({"fileType":"pdf","formatVersion":1,"pageCount":evidence.pages,"originalPageCount":evidence.pages,"pages":null,"sizeInBytes":pdf.len().to_string(),"coverPageNumber":-1,"documentMetadata":{},"extraMetadata":{},"fontName":"","lineHeight":-1,"margins":125,"orientation":"portrait","textAlignment":"justify","textScale":1,"zoomMode":"bestFit","tags":[],"pageTags":[]});
    let initialized = evidence.native_pages == INITIALIZED;
    if initialized {
        let order = page_order(evidence.pages)?;
        let rows=evidence.page_ids.iter().enumerate().map(|(i,id)|json!({"id":id,"idx":{"timestamp":"1:2","value":order[i]},"redir":{"timestamp":"1:2","value":i},"template":{"timestamp":"1:2","value":"Blank"}})).collect::<Vec<_>>();
        content["formatVersion"] = json!(2);
        content["cPages"] = json!({"original":{"timestamp":"1:1","value":evidence.pages},"pages":rows,"uuids":[{"first":author,"second":1}]});
        let object = content.as_object_mut().expect("constructed content object");
        object.remove("pages");
        object.remove("originalPageCount");
    }
    Ok(vec![
        FileUpdate {
            name: format!("{id}.pdf"),
            data: pdf,
        },
        FileUpdate {
            name: format!("{id}.pagedata"),
            data: if initialized { "Blank\n" } else { "\n" }
                .repeat(evidence.pages)
                .into_bytes(),
        },
        FileUpdate {
            name: format!("{id}.metadata"),
            data: serde_json::to_vec(&metadata)?,
        },
        FileUpdate {
            name: format!("{id}.content"),
            data: serde_json::to_vec(&content)?,
        },
    ])
}
pub async fn preflight(client: &Client, title: &str, folder: &str) -> Result<RootState> {
    let (root, manifest) = snapshot::root(client).await?;
    let mut metadata = HashMap::new();
    for entry in manifest.entries.iter().filter(|e| e.id != ".") {
        let bytes = client
            .get_blob_fresh(&entry.hash, &format!("{}.docSchema", entry.id))
            .await?;
        let files = parse_manifest(
            std::str::from_utf8(&bytes).map_err(|e| message(e.to_string()))?,
            &entry.hash,
        )?;
        snapshot::validate_manifest(&files)?;
        let meta = files
            .find(&format!("{}.metadata", entry.id))
            .ok_or_else(|| message("upload preflight item has no metadata"))?;
        let bytes = client.get_blob_fresh(&meta.hash, &meta.id).await?;
        if content_hash(&bytes) != meta.hash {
            return Err(message("upload preflight metadata checksum mismatch"));
        }
        let item: ItemMetadata = json::decode(&bytes)?;
        if !item.deleted && item.parent == folder && item.visible_name == title {
            return Err(message(format!(
                "title {title:?} already exists in destination folder ({})",
                entry.id
            )));
        }
        metadata.insert(entry.id.clone(), item);
    }
    let mut current = folder;
    let mut seen = HashSet::new();
    while !current.is_empty() {
        if !seen.insert(current) {
            return Err(message(
                "destination folder has a cycle in its parent chain",
            ));
        }
        let item = metadata
            .get(current)
            .ok_or_else(|| remarkable_cloud::Error::Domain {
                kind: ErrorKind::ItemNotFound,
                message: "destination folder or ancestor not found".into(),
            })?;
        if item.item_type != "CollectionType" {
            return Err(message(
                "destination folder or ancestor is not a collection",
            ));
        }
        if item.deleted || item.parent == "trash" {
            return Err(message("destination folder is deleted or in trash"));
        }
        current = &item.parent;
    }
    Ok(root)
}
pub fn write_evidence(path: &Path, evidence: &Evidence, exclusive: bool) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(evidence)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if exclusive {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    } else {
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| Error::Io(e.error))?;
    }
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn read_evidence(path: &Path) -> Result<Evidence> {
    let evidence: Evidence = json::decode(&std::fs::read(path)?)?;
    validate_evidence(&evidence)?;
    Ok(evidence)
}
pub fn validate_evidence(e: &Evidence) -> Result<()> {
    let valid_hash = |s: &str| hex::decode(s).is_ok_and(|v| v.len() == 32);
    if e.version != 1
        || validation::uuid(&e.result.id).is_err()
        || e.pages < 1
        || e.files.len() != 4
        || !valid_hash(&e.result.document_hash)
    {
        return Err(message(
            "upload evidence has incomplete or invalid recovery identity",
        ));
    }
    let mut seen = HashSet::new();
    for file in &e.files {
        let suffix = file.name.strip_prefix(&e.result.id).unwrap_or("");
        if ![".pdf", ".content", ".metadata", ".pagedata"].contains(&suffix)
            || !seen.insert(suffix)
            || file.size < 0
            || !valid_hash(&file.sha256)
        {
            return Err(message(format!(
                "invalid upload evidence file {:?}",
                file.name
            )));
        }
    }
    validate_native_pages(e, &e.result.id)
}
fn validate_native_pages(e: &Evidence, document_id: &str) -> Result<()> {
    if e.pages == 0 {
        return Err(message("upload requires at least one PDF page"));
    }
    match e.native_pages.as_str() {
        PENDING if e.page_ids.is_empty() => {}
        INITIALIZED => {
            page_order(e.pages)?;
            if e.page_ids.len() != e.pages {
                return Err(message("upload evidence native page count differs"));
            }
            let mut seen = HashSet::from([document_id]);
            for id in &e.page_ids {
                if validation::uuid(id).is_err() || !seen.insert(id) {
                    return Err(message(
                        "upload evidence has invalid or duplicate native page UUID",
                    ));
                }
            }
        }
        _ => return Err(message("upload evidence has invalid native_pages state")),
    }
    Ok(())
}
pub fn print(e: &Evidence, path: &Path, output: &Output, out: &mut dyn Write) -> Result<()> {
    let next = if e.native_pages == INITIALIZED {
        "Run doc inspect --pages, then doc import and doc settings transfer without opening the tablet first"
    } else {
        "Open on tablet, sync, then inspect --pages before doc import"
    };
    output.key_values(
        out,
        &[
            ("state", import::state(e.result.state).into()),
            ("document_id", e.result.id.clone()),
            ("title", e.title.clone()),
            ("folder", e.folder.clone()),
            ("pages", e.pages.to_string()),
            ("document_hash", e.result.document_hash.clone()),
            ("root_hash", e.result.root_hash.clone()),
            ("generation", e.result.generation.to_string()),
            ("evidence", output.path(path)),
            ("native_pages", e.native_pages.clone()),
            (
                "next_step",
                format!("{next}; use doc upload-check with this evidence before any retry"),
            ),
        ],
    )?;
    for name in &e.result.uploaded {
        writeln!(out, "uploaded: {name}")?;
    }
    Ok(())
}
pub async fn upload(
    client: &Client,
    path: &Path,
    options: Options<'_>,
    output: &Output,
    out: &mut dyn Write,
    commit: &crate::runtime::CommitProgress,
) -> Result<()> {
    let pdf = std::fs::read(path)?;
    let pages = remarkable_render::Document::from_bytes(pdf.clone())?.num_pages();
    if pages == 0 {
        return Err(message("upload PDF has no pages"));
    }
    if options.initialize_pages {
        page_order(pages)?;
    }
    let mut evidence = Evidence {
        version: 1,
        title: options.title.into(),
        folder: options.folder.into(),
        pages,
        native_pages: if options.initialize_pages {
            INITIALIZED
        } else {
            PENDING
        }
        .into(),
        page_ids: if options.initialize_pages {
            (0..pages)
                .map(|_| uuid::Uuid::new_v4().to_string())
                .collect()
        } else {
            vec![]
        },
        result: CreateResult::default(),
        files: vec![],
    };
    let root = preflight(client, options.title, options.folder).await?;
    let id = uuid::Uuid::new_v4().to_string();
    let files = files(&id, pdf, &evidence, &uuid::Uuid::new_v4().to_string())?;
    evidence.result = CreateResult {
        id: id.clone(),
        generation: root.generation,
        ..Default::default()
    };
    evidence.files = files
        .iter()
        .map(|f| UploadFile {
            name: f.name.clone(),
            sha256: content_hash(&f.data),
            size: f.data.len() as i64,
        })
        .collect();
    write_evidence(options.evidence, &evidence, true)?;
    let result = client
        .create_document_with_progress(
            &CreateDocumentOptions {
                id,
                expected_root: root,
                files,
            },
            &mut |progress| {
                commit.set(progress.state);
                evidence.result = progress;
                write_evidence(options.evidence, &evidence, false)
                    .and_then(|_| print(&evidence, options.evidence, output, out))
                    .map_err(|e| match e {
                        Error::Io(e) => remarkable_cloud::Error::Io(e),
                        e => remarkable_cloud::Error::Message(e.to_string()),
                    })
            },
        )
        .await;
    let error = match result {
        Ok(result) => {
            evidence.result = result;
            None
        }
        Err(failure) => {
            evidence.result = failure.progress;
            Some(Error::from(failure.error))
        }
    };
    commit.set(evidence.result.state);
    let persist = write_evidence(options.evidence, &evidence, false);
    let print = if error.is_some() {
        print(&evidence, options.evidence, output, out)
    } else {
        Ok(())
    };
    let final_error = match (persist, print) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(a), Err(b)) => Err(Error::Multiple(vec![a, b])),
    };
    import::finish(evidence.result.state, error, final_error)
}
pub fn check_identity(e: &Evidence, bytes: &[u8]) -> Result<()> {
    let fields = json::object(bytes)?;
    let ids = json::page_ids(&fields)?;
    if ids != e.page_ids
        || !matches!(json::value(&fields, "pages")?, Value::Null)
        || json::value(&fields, "pageCount")? != json!(e.pages)
    {
        return Err(message("upload recovery native page identity differs"));
    }
    let content = json::value(&fields, "cPages")?;
    let pages = content["pages"]
        .as_array()
        .ok_or_else(|| message("missing native pages"))?;
    for (i, page) in pages.iter().enumerate() {
        if page.get("deleted").is_some() || page["redir"]["value"] != json!(i) {
            return Err(message(format!(
                "upload recovery native page identity differs at page {i}"
            )));
        }
    }
    Ok(())
}
pub async fn check(client: &Client, e: &mut Evidence) -> Result<()> {
    let (root, manifest) = snapshot::root(client).await?;
    let document = snapshot::read(client, &manifest, &e.result.id).await?;
    if document.hash != e.result.document_hash || document.files.len() != e.files.len() {
        return Err(message(
            "upload recovery document association or file set differs",
        ));
    }
    for file in &e.files {
        let entry = document
            .manifest
            .find(&file.name)
            .ok_or_else(|| message("upload recovery file missing"))?;
        if entry.hash != file.sha256 || entry.size != file.size {
            return Err(message("upload recovery file association differs"));
        }
        if file.name == format!("{}.content", e.result.id) && e.native_pages == INITIALIZED {
            check_identity(e, &document.files[&file.name])?;
        }
    }
    snapshot::guard(client, &root).await?;
    e.result.state = UpdateState::Verified;
    Ok(())
}
