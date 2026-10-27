use crate::{
    error::{Result, message},
    output::Output,
    snapshot,
};
use remarkable_cloud::{Client, content_hash};
use serde_json::json;
use std::{io::Write, path::Path};
pub async fn archive(
    client: &Client,
    query: &str,
    path: &Path,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(message("archive output already exists")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let item = client.resolve(query).await?;
    if !item.is_document() {
        return Err(message("only documents can be archived"));
    }
    let (root, manifest) = snapshot::root(client).await?;
    if !manifest.find(&item.id).is_some_and(|e| e.hash == item.hash) {
        return Err(message("document changed since resolution"));
    }
    let document = snapshot::read(client, &manifest, &item.id).await?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut zip = zip::ZipWriter::new(temp.as_file_mut());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut files = Vec::new();
    let mut rows = Vec::new();
    for entry in &document.manifest.entries {
        let bytes = &document.files[&entry.id];
        let sha = content_hash(bytes);
        zip.start_file(format!("files/{}", entry.id), options)?;
        zip.write_all(bytes)?;
        files.push(json!({"name":entry.id,"hash":entry.hash,"sha256":sha,"size":bytes.len()}));
        rows.push(vec![
            entry.id.clone(),
            entry.hash.clone(),
            sha,
            bytes.len().to_string(),
        ]);
    }
    zip.start_file(format!("evidence/{}.docSchema", item.id), options)?;
    zip.write_all(&document.manifest_bytes)?;
    let hash = content_hash(&document.manifest_bytes);
    zip.start_file("evidence/snapshot.json", options)?;
    serde_json::to_writer_pretty(
        &mut zip,
        &json!({"format_version":1,"document_id":item.id,"document_hash":item.hash,"root":root,"manifest_sha256":hash,"files":files}),
    )?;
    zip.write_all(b"\n")?;
    zip.finish()?;
    snapshot::guard(client, &root).await?;
    temp.as_file().sync_all()?;
    std::fs::hard_link(temp.path(), path)?;
    output.key_values(
        out,
        &[
            ("state", "verified".into()),
            ("output", output.path(path)),
            ("document_id", item.id),
            ("document_hash", item.hash),
            ("root_hash", root.hash),
            ("generation", root.generation.to_string()),
            ("manifest_sha256", hash),
        ],
    )?;
    output.table(out, &["NAME", "HASH", "SHA256", "BYTES"], &rows)?;
    Ok(())
}
