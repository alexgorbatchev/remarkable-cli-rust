use crate::{
    error::{Error, Result, message},
    import,
    json::{self, Object},
    output::Output,
    snapshot,
};
use remarkable_cloud::{Client, FileUpdate, UpdateDocumentOptions, UpdateState};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::Path,
};
const VIEWPORT: [&str; 8] = [
    "zoomMode",
    "viewBackgroundFilter",
    "customZoomCenterX",
    "customZoomCenterY",
    "customZoomOrientation",
    "customZoomPageHeight",
    "customZoomPageWidth",
    "customZoomScale",
];
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageMap {
    pub source_page: usize,
    pub destination_page: usize,
}
#[derive(Clone, Debug)]
pub struct Difference {
    pub field: String,
    pub destination: Option<Box<RawValue>>,
    pub source: Option<Box<RawValue>>,
}
pub fn mapping(path: &Path) -> Result<Vec<PageMap>> {
    let rows: Vec<PageMap> = json::decode(&std::fs::read(path)?)?;
    validate_map(&rows)?;
    Ok(rows)
}
pub fn validate_map(rows: &[PageMap]) -> Result<()> {
    let (mut source, mut dest) = (HashSet::new(), HashSet::new());
    for row in rows {
        if !source.insert(row.source_page) || !dest.insert(row.destination_page) {
            return Err(message("mapping has duplicate source or destination pages"));
        }
    }
    Ok(())
}
fn viewport(fields: &Object) -> Result<()> {
    for key in VIEWPORT {
        let Some(raw) = fields.get(key) else { continue };
        let value: serde_json::Value = serde_json::from_str(raw.get())?;
        let valid = match key {
            "zoomMode" => value.as_str().is_some_and(|s| {
                ["bestFit", "customFit", "fitToHeight", "fitToWidth"].contains(&s)
            }),
            "viewBackgroundFilter" => value
                .as_str()
                .is_some_and(|s| ["off", "fullpage"].contains(&s)),
            "customZoomOrientation" => value
                .as_str()
                .is_some_and(|s| ["portrait", "landscape"].contains(&s)),
            _ => value.is_number() && raw.get().parse::<f64>().is_ok_and(f64::is_finite),
        };
        if !valid {
            return Err(message(format!("unsupported {key} value")));
        }
    }
    Ok(())
}
fn tags(fields: &Object, key: &str, ids: &[String]) -> Result<Vec<Object>> {
    let Some(raw) = fields.get(key) else {
        return Ok(vec![]);
    };
    let rows: Vec<Object> = json::decode(raw.get().as_bytes())?;
    for row in &rows {
        if !json::value(row, "name")?
            .as_str()
            .is_some_and(|s| !s.is_empty())
            || !json::value(row, "timestamp")?.is_number()
        {
            return Err(message(format!(
                "{key} requires named tags with numeric timestamps"
            )));
        }
        if key == "pageTags"
            && !json::value(row, "pageId")?
                .as_str()
                .is_some_and(|s| ids.iter().any(|id| id == s))
        {
            return Err(message("pageTags references unknown native page"));
        }
    }
    Ok(rows)
}
fn same_rows(a: &[Object], b: &[Object]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| json::same(a, b)))
        })
}
fn transfer_tags(
    source: &Object,
    dest: &mut Object,
    source_ids: &[String],
    dest_ids: &[String],
    map: &[PageMap],
) -> Result<()> {
    let tags = tags(source, "tags", source_ids)?;
    let dst_tags = self::tags(dest, "tags", dest_ids)?;
    let src_pages = self::tags(source, "pageTags", source_ids)?;
    let mut retained = self::tags(dest, "pageTags", dest_ids)?;
    let page_map: HashMap<_, _> = map
        .iter()
        .map(|m| (&source_ids[m.source_page], &dest_ids[m.destination_page]))
        .collect();
    let mut remapped: HashMap<String, Vec<Object>> = HashMap::new();
    for mut tag in src_pages {
        let id: String = serde_json::from_value(json::value(&tag, "pageId")?)?;
        let destination = page_map
            .get(&id)
            .ok_or_else(|| message(format!("source tagged native page {id} has no mapping")))?;
        tag.insert("pageId".into(), json::raw(destination)?);
        remapped
            .entry(destination.to_string())
            .or_default()
            .push(tag);
    }
    let mut existing: HashMap<String, Vec<Object>> = HashMap::new();
    for tag in &retained {
        let id = serde_json::from_value(json::value(tag, "pageId")?)?;
        existing.entry(id).or_default().push(tag.clone());
    }
    let mut conflicts = Vec::new();
    if !dst_tags.is_empty() && !same_rows(&tags, &dst_tags) {
        conflicts.push(message(
            "destination document tags conflict with source tags",
        ));
    }
    for row in map {
        let id = &dest_ids[row.destination_page];
        let incoming = remapped.get(id).map_or(&[][..], Vec::as_slice);
        if let Some(current) = existing.get(id).filter(|v| !v.is_empty()) {
            if !same_rows(current, incoming) {
                conflicts.push(message(format!(
                    "destination page {} ({id}) tags conflict",
                    row.destination_page
                )));
            }
        } else {
            retained.extend_from_slice(incoming);
        }
    }
    if !conflicts.is_empty() {
        return Err(Error::Multiple(conflicts));
    }
    if !tags.is_empty() || source.contains_key("tags") || dest.contains_key("tags") {
        dest.insert("tags".into(), json::raw(&tags)?);
    }
    if !retained.is_empty() || source.contains_key("pageTags") || dest.contains_key("pageTags") {
        dest.insert("pageTags".into(), json::raw(&retained)?);
    }
    Ok(())
}
pub fn merge(
    source: &Object,
    destination: &Object,
    map: &[PageMap],
    replace: bool,
    differences: &mut Vec<Difference>,
) -> Result<Vec<u8>> {
    validate_map(map)?;
    let source_ids = json::page_ids(source)?;
    let dest_ids = json::page_ids(destination)?;
    viewport(source)?;
    viewport(destination)?;
    for row in map {
        if row.source_page >= source_ids.len() || row.destination_page >= dest_ids.len() {
            return Err(message(format!(
                "mapping {} -> {} is out of range",
                row.source_page, row.destination_page
            )));
        }
    }
    let mut dest = destination.clone();
    let mut conflicts = Vec::new();
    if let Err(e) = transfer_tags(source, &mut dest, &source_ids, &dest_ids, map) {
        conflicts.push(e);
    }
    for key in VIEWPORT {
        let a = source.get(key);
        let b = destination.get(key);
        if match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => json::same(a, b),
            _ => false,
        } {
            continue;
        }
        differences.push(Difference {
            field: key.into(),
            destination: b.cloned(),
            source: a.cloned(),
        });
        if !replace {
            conflicts.push(message(format!("destination viewport {key} differs; review differences and use --replace-viewport to replace it")));
        }
    }
    if !conflicts.is_empty() {
        return Err(Error::Multiple(conflicts));
    }
    for key in VIEWPORT {
        if let Some(raw) = source.get(key) {
            dest.insert(key.into(), raw.clone());
        } else {
            dest.remove(key);
        }
    }
    Ok(serde_json::to_vec(&dest)?)
}
struct Progress {
    state: UpdateState,
    source: String,
    dest: String,
    source_hash: String,
    dest_hash: String,
    root_hash: String,
    generation: i64,
    uploaded: Vec<String>,
    differences: Vec<Difference>,
}
fn print(p: &Progress, output: &Output, out: &mut dyn Write) -> Result<()> {
    output.key_values(
        out,
        &[
            ("state", import::state(p.state).into()),
            ("source_id", p.source.clone()),
            ("destination_id", p.dest.clone()),
            ("source_hash", p.source_hash.clone()),
            ("destination_hash", p.dest_hash.clone()),
            ("root_hash", p.root_hash.clone()),
            ("generation", p.generation.to_string()),
        ],
    )?;
    for name in &p.uploaded {
        output.key_values(out, &[("uploaded", name.clone())])?;
    }
    if !p.differences.is_empty() {
        output.table(
            out,
            &[
                "FIELD",
                "DESTINATION PRESENT",
                "DESTINATION VALUE",
                "SOURCE PRESENT",
                "SOURCE VALUE",
            ],
            &p.differences
                .iter()
                .map(|d| {
                    vec![
                        d.field.clone(),
                        d.destination.is_some().to_string(),
                        d.destination.as_ref().map_or("absent", |v| v.get()).into(),
                        d.source.is_some().to_string(),
                        d.source.as_ref().map_or("absent", |v| v.get()).into(),
                    ]
                })
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(())
}
pub struct TransferOptions<'a> {
    pub source: &'a str,
    pub destination: &'a str,
    pub mapping: &'a [PageMap],
    pub replace_viewport: bool,
}
pub async fn transfer(
    client: &Client,
    options: TransferOptions<'_>,
    output: &Output,
    out: &mut dyn Write,
    commit: &crate::runtime::CommitProgress,
) -> Result<()> {
    let TransferOptions {
        source,
        destination: dest,
        mapping: map,
        replace_viewport: replace,
    } = options;
    crate::validation::uuid(source).map_err(message)?;
    crate::validation::uuid(dest).map_err(message)?;
    if source.eq_ignore_ascii_case(dest) {
        return Err(message("source and destination must be separate documents"));
    }
    validate_map(map)?;
    let (root, manifest) = snapshot::root(client).await?;
    let src = snapshot::read(client, &manifest, source).await?;
    let dst = snapshot::read(client, &manifest, dest).await?;
    let mut progress = Progress {
        state: UpdateState::Staged,
        source: source.into(),
        dest: dest.into(),
        source_hash: src.hash.clone(),
        dest_hash: dst.hash.clone(),
        root_hash: String::new(),
        generation: root.generation,
        uploaded: vec![],
        differences: vec![],
    };
    let operation = async {
        let src_fields = json::object(&src.files[&format!("{source}.content")])?;
        let original = &dst.files[&format!("{dest}.content")];
        let dest_fields = json::object(original)?;
        let mut content = merge(
            &src_fields,
            &dest_fields,
            map,
            replace,
            &mut progress.differences,
        )?;
        if !json::same(
            &RawValue::from_string(
                String::from_utf8(content.clone()).map_err(|e| message(e.to_string()))?,
            )?,
            &RawValue::from_string(
                String::from_utf8(original.clone()).map_err(|e| message(e.to_string()))?,
            )?,
        ) {
            let options = UpdateDocumentOptions {
                id: dest.into(),
                expected_hash: dst.hash.clone(),
                expected_root: root.clone(),
                files: vec![FileUpdate {
                    name: format!("{dest}.content"),
                    data: content.clone(),
                }],
            };
            let (result, error) = match client
                .update_document_files_at_root_with_progress(&options, &mut |p| {
                    commit.set(if p.state == UpdateState::Verified {
                        UpdateState::Committed
                    } else {
                        p.state
                    });
                    Ok(())
                })
                .await
            {
                Ok(result) => (result, None),
                Err(failure) => (failure.progress, Some(failure.error)),
            };
            progress.state = result.state;
            progress.uploaded = result.uploaded;
            progress.root_hash = result.root_hash;
            if let Some(error) = error {
                commit.set(progress.state);
                return Err(error.into());
            }
            progress.state = UpdateState::Committed;
        } else {
            content = original.clone();
            progress.root_hash = root.hash.clone();
        }
        let (after, manifest) = snapshot::root(client).await?;
        if after.hash != progress.root_hash
            || (progress.state == UpdateState::Staged && after.generation != progress.generation)
        {
            return Err(message(
                "cloud root changed after settings preflight/commit",
            ));
        }
        snapshot::verify(client, &manifest, &src, None).await?;
        let current = snapshot::verify(client, &manifest, &dst, Some(&content)).await?;
        progress.dest_hash = current.hash;
        snapshot::guard(client, &after).await?;
        progress.state = UpdateState::Verified;
        Ok(())
    }
    .await;
    commit.set(progress.state);
    import::finish(
        progress.state,
        operation.err(),
        print(&progress, output, out),
    )
}
