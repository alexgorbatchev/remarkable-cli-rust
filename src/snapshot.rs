use crate::{
    error::{Result, message},
    json, validation,
};
use remarkable_cloud::{
    Client, ErrorKind, Manifest, RootState, content_hash, encode_cloud_manifest, parse_manifest,
};
use std::collections::{BTreeMap, HashSet};
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub hash: String,
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub files: BTreeMap<String, Vec<u8>>,
}
pub async fn root(client: &Client) -> Result<(RootState, Manifest)> {
    let root = client.get_root_state().await?;
    let bytes = client.get_blob_fresh(&root.hash, "root.docSchema").await?;
    if content_hash(&bytes) != root.hash {
        return Err(message("root manifest hash mismatch"));
    }
    let manifest = parse_manifest(
        std::str::from_utf8(&bytes).map_err(|e| message(e.to_string()))?,
        &root.hash,
    )?;
    let mut seen = HashSet::new();
    if manifest.entries.iter().any(|e| !seen.insert(&e.id)) {
        return Err(message("duplicate root entry"));
    }
    Ok((root, manifest))
}
pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    let mut seen = HashSet::new();
    for entry in &manifest.entries {
        if !validation::safe_native_path(&entry.id)
            || entry.size < 0
            || hex::decode(&entry.hash).map_or(true, |v| v.len() != 32)
            || !seen.insert(&entry.id)
        {
            return Err(message(format!(
                "invalid or duplicate native manifest entry {:?}",
                entry.id
            )));
        }
    }
    if encode_cloud_manifest(&manifest.entries, false)?.hash != manifest.hash {
        return Err(message("document manifest hash mismatch"));
    }
    Ok(())
}
pub async fn read(client: &Client, root: &Manifest, id: &str) -> Result<Snapshot> {
    let entry = root
        .find(id)
        .ok_or_else(|| remarkable_cloud::Error::Domain {
            kind: ErrorKind::ItemNotFound,
            message: format!("document {id} missing from root"),
        })?;
    let bytes = client
        .get_blob_fresh(&entry.hash, &format!("{id}.docSchema"))
        .await?;
    let manifest = parse_manifest(
        std::str::from_utf8(&bytes).map_err(|e| message(e.to_string()))?,
        &entry.hash,
    )?;
    validate_manifest(&manifest)?;
    let mut files = BTreeMap::new();
    for entry in &manifest.entries {
        let data = client.get_blob_fresh(&entry.hash, &entry.id).await?;
        if content_hash(&data) != entry.hash || data.len() as i64 != entry.size {
            return Err(message(format!("file {} hash or size mismatch", entry.id)));
        }
        files.insert(entry.id.clone(), data);
    }
    if !files.contains_key(&format!("{id}.content")) {
        return Err(message("document content missing"));
    }
    let metadata = files
        .get(&format!("{id}.metadata"))
        .ok_or_else(|| message("document metadata missing"))?;
    if json::value(&json::object(metadata)?, "type")? != "DocumentType" {
        return Err(message(format!("{id} is not a document")));
    }
    Ok(Snapshot {
        id: id.into(),
        hash: entry.hash.clone(),
        manifest,
        manifest_bytes: bytes,
        files,
    })
}
pub async fn verify(
    client: &Client,
    root: &Manifest,
    before: &Snapshot,
    content: Option<&[u8]>,
) -> Result<Snapshot> {
    if content.is_none() && !root.find(&before.id).is_some_and(|e| e.hash == before.hash) {
        return Err(message("source document changed during transfer"));
    }
    let current = read(client, root, &before.id).await?;
    if current.manifest.entries.len() != before.manifest.entries.len() {
        return Err(message("document file set changed"));
    }
    for entry in &before.manifest.entries {
        let wanted = if entry.id == format!("{}.content", before.id) {
            content.unwrap_or(&before.files[&entry.id])
        } else {
            &before.files[&entry.id]
        };
        if !current.manifest.find(&entry.id).is_some_and(|e| {
            e.hash == content_hash(wanted)
                && e.size == wanted.len() as i64
                && e.entry_type == entry.entry_type
                && e.subfiles == entry.subfiles
        }) || current.files[&entry.id] != wanted
        {
            return Err(message(format!(
                "document preservation verification failed for {}",
                entry.id
            )));
        }
    }
    Ok(current)
}
pub async fn guard(client: &Client, expected: &RootState) -> Result<()> {
    if client.get_root_state().await? != *expected {
        return Err(message("root changed during verification"));
    }
    Ok(())
}
