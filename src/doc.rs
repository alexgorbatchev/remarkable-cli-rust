use crate::{
    error::{Error, Result, message},
    output::{Output, TreeNode, printable_text, printable_uri},
    validation,
};
use remarkable_cloud::{Client, DocumentContent, Item, ListOptions, Manifest};
use remarkable_render::{Document, PageLink};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    path::Path,
};
pub fn sort_items(items: &mut [Item]) {
    let by_id: HashMap<_, _> = items
        .iter()
        .filter(|i| i.is_collection())
        .map(|i| (i.id.clone(), i.clone()))
        .collect();
    let fold = icu_casemap::CaseMapper::new();
    let key = |i: &Item| {
        (
            fold.fold_string(&i.metadata.visible_name).into_owned(),
            i.metadata.visible_name.clone(),
            i.id.clone(),
        )
    };
    let path = |item: &Item| {
        let mut parent = item.metadata.parent.as_str();
        let mut seen = HashSet::new();
        let mut chain = Vec::new();
        while !parent.is_empty() {
            if !seen.insert(parent) {
                return None;
            }
            let folder = by_id.get(parent)?;
            chain.push(key(folder));
            parent = &folder.metadata.parent;
        }
        chain.reverse();
        Some(chain)
    };
    let paths: HashMap<_, _> = items.iter().map(|i| (i.id.clone(), path(i))).collect();
    items.sort_by(|a, b| match (&paths[&a.id], &paths[&b.id]) {
        (Some(a_path), Some(b_path)) => a_path.cmp(b_path).then_with(|| key(a).cmp(&key(b))),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a
            .metadata
            .parent
            .cmp(&b.metadata.parent)
            .then_with(|| key(a).cmp(&key(b))),
    });
}
pub async fn list(
    client: &Client,
    folder: &str,
    kind: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<Item>> {
    let mut items = client.list_items(ListOptions::default()).await?;
    sort_items(&mut items);
    let fold = icu_casemap::CaseMapper::new();
    let query = fold.fold_string(query.trim());
    Ok(items
        .into_iter()
        .filter(|i| {
            (folder.is_empty() || i.metadata.parent == folder)
                && (kind.is_empty() || i.metadata.item_type == kind)
                && (query.is_empty()
                    || fold
                        .fold_string(&i.metadata.visible_name)
                        .contains(query.as_ref()))
        })
        .take(if limit > 0 {
            limit as usize
        } else {
            usize::MAX
        })
        .collect())
}
pub fn tree(items: &[Item]) -> TreeNode {
    fn children(items: &[Item], parent: &str, seen: &mut HashSet<String>) -> Vec<TreeNode> {
        items
            .iter()
            .filter(|i| i.metadata.parent == parent)
            .filter_map(|i| {
                if !seen.insert(i.id.clone()) {
                    return None;
                }
                Some(TreeNode {
                    label: format!(
                        "{}{}",
                        i.metadata.visible_name,
                        if i.is_collection() { "/" } else { "" }
                    ),
                    children: if i.is_collection() {
                        children(items, &i.id, seen)
                    } else {
                        vec![]
                    },
                })
            })
            .collect()
    }
    TreeNode {
        label: "/".into(),
        children: children(items, "", &mut HashSet::new()),
    }
}
pub fn page_ids(content: &DocumentContent) -> Vec<String> {
    if !content.pages.is_empty() {
        content.pages.clone()
    } else {
        content
            .c_pages
            .pages
            .iter()
            .filter(|p| !p.id.is_empty())
            .map(|p| p.id.clone())
            .collect()
    }
}
fn native_page(ids: &[String], index: usize) -> Result<&str> {
    ids.get(index).map(String::as_str).ok_or(Error::Page {
        index,
        pages: ids.len(),
    })
}
pub async fn pdf(client: &Client, item: &Item, manifest: &Manifest) -> Result<Vec<u8>> {
    let entry = manifest
        .find(&format!("{}.pdf", item.id))
        .or_else(|| manifest.find_suffix(".pdf"))
        .ok_or_else(|| {
            message(format!(
                "document {:?} has no background PDF",
                item.metadata.visible_name
            ))
        })?;
    Ok(client
        .get_blob(&entry.hash, &format!("{}.pdf", item.id))
        .await?)
}
pub async fn strokes(
    client: &Client,
    item: &Item,
    manifest: &Manifest,
    page_id: &str,
) -> Result<Option<Vec<u8>>> {
    let name = format!("{}/{page_id}.rm", item.id);
    let entry = manifest
        .find(&name)
        .or_else(|| manifest.find_suffix(&format!("{page_id}.rm")));
    match entry {
        None => Ok(None),
        Some(entry) => Ok(Some(client.get_blob(&entry.hash, &name).await?)),
    }
}
pub async fn inspect(
    client: &Client,
    query: &str,
    pages: bool,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let mut ids = Vec::new();
    let mut format = String::new();
    if !item.is_collection() {
        let content = item.get_content().await?;
        ids = page_ids(&content);
        format = content.file_type;
    }
    output.key_values(
        out,
        &[
            ("ID", item.id.clone()),
            ("Name", item.metadata.visible_name.clone()),
            ("Type", item.metadata.item_type.clone()),
            ("Format", format),
            ("Pages", ids.len().to_string()),
            ("Modified", item.metadata.last_modified.clone()),
        ],
    )?;
    if pages && !ids.is_empty() {
        let manifest = item.get_manifest().await?;
        let rows = ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let presence = manifest.find_suffix(&format!("{id}.rm")).map_or_else(
                    || "empty".into(),
                    |entry| format!("present ({} bytes)", entry.size),
                );
                vec![index.to_string(), id.clone(), presence]
            })
            .collect::<Vec<_>>();
        if output.mode == cli_command_kit::Mode::Human {
            writeln!(out)?;
        }
        output.table(out, &["PAGE", "PAGE ID", "STROKES"], &rows)?;
    }
    Ok(())
}
pub async fn cat(
    client: &Client,
    query: &str,
    page: usize,
    format: &str,
    out: &mut dyn Write,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let manifest = item.get_manifest().await?;
    let bytes = match format {
        "pdf" => pdf(client, &item, &manifest).await?,
        "text" => Document::from_bytes(pdf(client, &item, &manifest).await?)?
            .text(page as isize)?
            .into_bytes(),
        _ => {
            let ids = page_ids(&item.get_content().await?);
            let id = native_page(&ids, page)?;
            let bytes = strokes(client, &item, &manifest, id)
                .await?
                .ok_or_else(|| message(format!("no stroke data found for page {page} ({id})")))?;
            if format == "rm" {
                bytes
            } else {
                rmscene::render_rm_to_svg(&bytes, &Default::default())?.into_bytes()
            }
        }
    };
    out.write_all(&bytes)?;
    Ok(())
}
pub async fn render(
    client: &Client,
    query: &str,
    page: usize,
    dpi: i32,
    path: &Path,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let manifest = item.get_manifest().await?;
    let pdf = pdf(client, &item, &manifest).await?;
    let ids = page_ids(&item.get_content().await?);
    let id = native_page(&ids, page)?;
    let rm = strokes(client, &item, &manifest, id)
        .await?
        .unwrap_or_default();
    let png = remarkable_render::render_planner_page(
        &pdf,
        page as isize,
        &rm,
        if dpi <= 0 { 200 } else { dpi },
    )?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, png)?;
    Ok(())
}
pub struct SyncOptions<'a> {
    pub output_dir: &'a Path,
    pub format: &'a str,
    pub dpi: i32,
    pub force: bool,
}
pub async fn sync(
    client: &Client,
    query: &str,
    options: SyncOptions<'_>,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let ids = page_ids(&item.get_content().await?);
    let manifest = item.get_manifest().await?;
    let pdf = if options.format == "png" && manifest.find_suffix(".pdf").is_some() {
        pdf(client, &item, &manifest).await?
    } else {
        Vec::new()
    };
    let dir = options.output_dir.join(validation::safe_document_dir(
        &item.metadata.visible_name,
        &item.id,
    ));
    let mut results = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let path = dir.join(format!("page-{index:03}.{}", options.format));
        if !options.force {
            match std::fs::metadata(&path) {
                Ok(_) => {
                    results.push(("skipped", path));
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        let rm = strokes(client, &item, &manifest, id)
            .await?
            .unwrap_or_default();
        let bytes = match options.format {
            "rm" => rm,
            "svg" => rmscene::render_rm_to_svg(&rm, &Default::default())?.into_bytes(),
            _ => remarkable_render::render_planner_page(
                &pdf,
                index as isize,
                &rm,
                if options.dpi <= 0 { 200 } else { options.dpi },
            )?,
        };
        std::fs::create_dir_all(&dir)?;
        std::fs::write(&path, bytes)?;
        results.push(("written", path));
    }
    let (mut written, mut skipped) = (0, 0);
    for (state, path) in results {
        if state == "written" {
            written += 1
        } else {
            skipped += 1
        };
        let path = output.path(&path);
        if output.mode == cli_command_kit::Mode::Agent {
            writeln!(out, "{state}: {}", printable_text(path.as_bytes()))?;
        } else {
            output.status(
                out,
                if state == "written" { "ok" } else { "info" },
                &format!(
                    "{} {}",
                    if state == "written" {
                        "Synced"
                    } else {
                        "Skipped existing"
                    },
                    printable_text(path.as_bytes())
                ),
            )?;
        }
    }
    if output.mode == cli_command_kit::Mode::Human {
        output.separator(out)?;
        output.status(
            out,
            "ok",
            &format!("Sync complete: {written} written, {skipped} skipped"),
        )?;
    }
    Ok(())
}
pub async fn search(
    client: &Client,
    query: &str,
    text: &str,
    whole: bool,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let manifest = item.get_manifest().await?;
    let doc = Document::from_bytes(pdf(client, &item, &manifest).await?)?;
    let mut rows = Vec::new();
    for page in 0..doc.num_pages() {
        if let Some(snippet) = crate::search::snippet(&doc.text(page as isize)?, text, whole) {
            rows.push(vec![page.to_string(), snippet]);
        }
    }
    if output.mode == cli_command_kit::Mode::Agent {
        output.table(out, &[], &rows)?;
    } else if rows.is_empty() {
        output.status(out, "info", &format!("No matches found for {text:?}"))?;
    } else {
        output.table(out, &["PAGE", "SNIPPET"], &rows)?;
    }
    Ok(())
}
pub fn link_fields(link: &PageLink) -> Vec<String> {
    fn coordinate(v: f64) -> String {
        let v = v as f32;
        if v == f32::INFINITY {
            "+Inf".into()
        } else if v == f32::NEG_INFINITY {
            "-Inf".into()
        } else {
            v.to_string()
        }
    }
    vec![
        link.index.to_string(),
        link.target_page
            .map_or_else(|| "-".into(), |p| p.to_string()),
        printable_uri(&link.uri),
        link.text.clone(),
        [
            link.rect.left,
            link.rect.bottom,
            link.rect.right,
            link.rect.top,
        ]
        .map(coordinate)
        .join(","),
    ]
}
pub async fn links(
    client: &Client,
    query: &str,
    page: usize,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let item = client.resolve(query).await?;
    let manifest = item.get_manifest().await?;
    let doc = Document::from_bytes(pdf(client, &item, &manifest).await?)?;
    let links = doc.links(page as isize)?;
    let rows = links.iter().map(link_fields).collect::<Vec<_>>();
    if output.mode == cli_command_kit::Mode::Agent {
        output.table(out, &[], &rows)?;
    } else if rows.is_empty() {
        output.status(out, "info", &format!("No hyperlinks found on page {page}"))?;
    } else {
        output.table(
            out,
            &["LINK #", "TARGET PAGE", "URI", "TEXT", "RECT"],
            &rows,
        )?;
    }
    Ok(())
}
pub fn stroke_inspect(path: &Path, output: &Output, out: &mut dyn Write) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let scene = rmscene::parse(&bytes)?;
    let mut tools = BTreeMap::<String, usize>::new();
    let mut colors = BTreeMap::<String, usize>::new();
    let mut points = 0;
    let mut lines = 0;
    for line in scene.line_items().filter_map(|i| i.item.value.as_ref()) {
        lines += 1;
        points += line.points.len();
        *tools.entry(line.tool.to_string()).or_default() += 1;
        *colors.entry(line.color.to_string()).or_default() += 1;
    }
    let mut pairs = vec![
        ("File".to_string(), output.path(path)),
        ("File Size".into(), format!("{} bytes", bytes.len())),
        ("Total Blocks".into(), scene.blocks.len().to_string()),
        ("Lines".into(), lines.to_string()),
        ("Points".into(), points.to_string()),
    ];
    pairs.extend(
        tools
            .into_iter()
            .map(|(name, n)| (format!("Tool: {name}"), format!("{n} lines"))),
    );
    pairs.extend(
        colors
            .into_iter()
            .map(|(name, n)| (format!("Color: {name}"), format!("{n} lines"))),
    );
    output.key_values(
        out,
        &pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}
pub fn stroke_export(
    path: &Path,
    width: f64,
    height: f64,
    destination: &str,
    output: &Output,
    out: &mut dyn Write,
) -> Result<()> {
    let mut options = rmscene::SvgOptions::default();
    if width > 0.0 && height > 0.0 {
        options.width_pt = width;
        options.height_pt = height;
    }
    let svg = rmscene::render_rm_to_svg(&std::fs::read(path)?, &options)?;
    if destination.is_empty() {
        out.write_all(svg.as_bytes())?;
    } else {
        std::fs::write(destination, svg)?;
        output.status(
            out,
            "ok",
            &format!("Exported SVG to {}", output.path(Path::new(destination))),
        )?;
    }
    Ok(())
}
