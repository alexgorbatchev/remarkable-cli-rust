use crate::{
    error::{Error, Result, after_commit, message},
    json,
    output::Output,
};
use remarkable_cloud::{Client, FileUpdate, UpdateState};
use serde::Deserialize;
use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageImport {
    pub source: PathBuf,
    pub page: usize,
}
pub fn mapping(path: &Path) -> Result<Vec<PageImport>> {
    let mut rows: Vec<PageImport> = json::decode(&std::fs::read(path)?)?;
    if rows.is_empty() {
        return Err(message("mapping must contain at least one page"));
    }
    for row in &mut rows {
        if row.source.as_os_str().is_empty() {
            return Err(message("mapping source must not be empty"));
        }
        if row.source.is_relative() {
            row.source = path.parent().unwrap_or(Path::new(".")).join(&row.source);
        }
    }
    Ok(rows)
}
pub fn empty_native(bytes: &[u8]) -> Result<()> {
    for block in rmscene::parse(bytes)?.blocks {
        match block.data {
            rmscene::BlockData::Migration(_)
            | rmscene::BlockData::Tree(_)
            | rmscene::BlockData::Node(_)
            | rmscene::BlockData::Group(_)
            | rmscene::BlockData::Authors(_)
            | rmscene::BlockData::SceneInfo(_) => {}
            rmscene::BlockData::Page(page)
                if page.text_chars_count == 0 && page.text_lines_count == 0 => {}
            _ => {
                return Err(message(
                    "destination contains handwriting, text, annotations, or unsupported native blocks",
                ));
            }
        }
    }
    Ok(())
}
pub async fn import(
    client: &Client,
    id: &str,
    rows: &[PageImport],
    output: &Output,
    out: &mut dyn Write,
    progress: &crate::runtime::CommitProgress,
) -> Result<()> {
    let item = client.resolve_by_id(id).await?;
    if !item.is_document() {
        return Err(message("destination is not a document"));
    }
    let manifest = item.get_manifest().await?;
    let content = manifest
        .find(&format!("{id}.content"))
        .ok_or_else(|| message("destination content missing"))?;
    let ids = json::page_ids(&json::object(
        &client.get_blob_fresh(&content.hash, &content.id).await?,
    )?)?;
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut pages = Vec::new();
    let mut failures = Vec::new();
    for row in rows {
        let result = async {
            if row.page >= ids.len() || !seen.insert(row.page) {
                return Err(message(format!(
                    "page {} has duplicate or out-of-range mapping",
                    row.page
                )));
            }
            let bytes = std::fs::read(&row.source)?;
            rmscene::parse(&bytes)?;
            let page_id = &ids[row.page];
            let basename = format!("{page_id}.rm");
            let name = format!("{id}/{basename}");
            for entry in &manifest.entries {
                if (entry.id == basename || entry.id.ends_with(&format!("/{basename}")))
                    && entry.id != name
                {
                    return Err(message("incompatible native page association"));
                }
            }
            if let Some(entry) = manifest.find(&name) {
                empty_native(&client.get_blob_fresh(&entry.hash, &name).await?)?;
            }
            files.push(FileUpdate { name, data: bytes });
            pages.push(vec![
                row.page.to_string(),
                page_id.clone(),
                output.path(&row.source),
            ]);
            Ok(())
        }
        .await;
        if let Err(error) = result {
            failures.push(error.context(format!(
                "page {} source {}",
                row.page,
                output.path(&row.source)
            )));
        }
    }
    if !failures.is_empty() {
        return Err(Error::Multiple(failures));
    }
    let entry = manifest
        .find(&format!("{id}.metadata"))
        .ok_or_else(|| message("destination metadata missing"))?;
    let mut metadata = json::object(&client.get_blob_fresh(&entry.hash, &entry.id).await?)?;
    metadata.insert(
        "lastModified".into(),
        json::raw(&crate::upload::now_millis().to_string())?,
    );
    files.push(FileUpdate {
        name: entry.id.clone(),
        data: serde_json::to_vec(&metadata)?,
    });
    let (result, error) = match client
        .update_document_files_with_progress(id, &item.hash, &files, &mut |result| {
            progress.set(result.state);
            Ok(())
        })
        .await
    {
        Ok(result) => (result, None),
        Err(failure) => (failure.progress, Some(Error::from(failure.error))),
    };
    progress.set(result.state);
    let print = (|| -> Result<()> {
        output.key_values(out, &[("state", state(result.state).into())])?;
        for name in &result.uploaded {
            output.key_values(out, &[("uploaded", name.clone())])?;
        }
        let rows = pages
            .into_iter()
            .map(|mut p| {
                p.push(state(result.state).into());
                p
            })
            .collect::<Vec<_>>();
        output.table(out, &["PAGE", "PAGE ID", "SOURCE", "STATE"], &rows)?;
        Ok(())
    })();
    finish(result.state, error, print)
}
pub fn state(state: UpdateState) -> &'static str {
    match state {
        UpdateState::Staged => "staged",
        UpdateState::CommitUnknown => "commit-unknown",
        UpdateState::Committed => "committed",
        UpdateState::Verified => "verified",
    }
}
pub fn finish(state: UpdateState, error: Option<Error>, print: Result<()>) -> Result<()> {
    match (error, print) {
        (None, Ok(())) => Ok(()),
        (Some(e), Ok(())) | (None, Err(e)) => Err(after_commit(state, e)),
        (Some(a), Err(b)) => Err(after_commit(state, Error::Multiple(vec![a, b]))),
    }
}
