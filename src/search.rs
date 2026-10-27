use unicode_general_category::{GeneralCategory as G, get_general_category};
fn word(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        G::UppercaseLetter
            | G::LowercaseLetter
            | G::TitlecaseLetter
            | G::ModifierLetter
            | G::OtherLetter
            | G::DecimalNumber
            | G::NonspacingMark
            | G::SpacingMark
            | G::EnclosingMark
    )
}
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn snippet(text: &str, query: &str, whole: bool) -> Option<String> {
    let query = normalize(query);
    if query.is_empty() {
        return None;
    }
    let mut normalized = Vec::new();
    let mut end = 0;
    for (offset, ch) in text.char_indices() {
        if !ch.is_whitespace() {
            if end > 0 && end < offset {
                normalized.push((' ', end, end));
            }
            normalized.push((ch, offset, offset + ch.len_utf8()));
            end = offset + ch.len_utf8();
        }
    }
    let fold = icu_casemap::CaseMapper::new();
    let query: Vec<_> = query.chars().map(|ch| fold.simple_fold(ch)).collect();
    for start in 0..normalized.len() {
        let end = start + query.len();
        if end > normalized.len() {
            break;
        }
        if !normalized[start..end]
            .iter()
            .map(|(ch, _, _)| fold.simple_fold(*ch))
            .eq(query.iter().copied())
        {
            continue;
        }
        if whole
            && ((start > 0 && word(normalized[start - 1].0))
                || (end < normalized.len() && word(normalized[end].0)))
        {
            continue;
        }
        let begin = normalized[start].1;
        let end = normalized[end - 1].2;
        let begin = text[..begin]
            .char_indices()
            .rev()
            .nth(19)
            .map_or(0, |(i, _)| i);
        let end = text[end..]
            .char_indices()
            .nth(40)
            .map_or(text.len(), |(i, _)| end + i);
        return Some(normalize(&text[begin..end]));
    }
    None
}
