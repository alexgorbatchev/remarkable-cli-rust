use std::path::Path;

pub fn nonempty(value: &str) -> std::result::Result<String, String> {
    if value.is_empty() {
        Err("path must not be empty".into())
    } else {
        Ok(value.into())
    }
}
pub fn uuid(value: &str) -> std::result::Result<String, String> {
    if value.len() == 36
        && [8, 13, 18, 23].iter().all(|i| value.as_bytes()[*i] == b'-')
        && uuid::Uuid::parse_str(value).is_ok()
    {
        Ok(value.into())
    } else {
        Err("document identity must be a UUID".into())
    }
}
pub fn folder(value: &str) -> std::result::Result<String, String> {
    if value.is_empty() {
        Ok(String::new())
    } else {
        uuid(value)
    }
}
pub fn title(value: &str) -> std::result::Result<String, String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err("upload title must be nonempty and contain no control characters".into())
    } else {
        Ok(value.into())
    }
}
pub fn query(value: &str) -> std::result::Result<String, String> {
    if value.trim().is_empty() {
        Err("query must not be empty or only whitespace".into())
    } else {
        Ok(value.into())
    }
}
pub fn pairing_code(value: &str) -> std::result::Result<String, String> {
    let value = value.trim();
    if value.len() == 8 {
        Ok(value.into())
    } else {
        Err(format!(
            "pairing code must be exactly 8 characters (got {})",
            value.len()
        ))
    }
}
pub fn cat_format(value: &str) -> std::result::Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if ["pdf", "text", "rm", "svg"].contains(&value.as_str()) {
        Ok(value)
    } else {
        Err("unsupported format (choose from: pdf, text, rm, svg)".into())
    }
}
pub fn sync_format(value: &str) -> std::result::Result<String, String> {
    if value.is_empty() {
        Ok("png".into())
    } else if ["png", "svg", "rm"].contains(&value) {
        Ok(value.into())
    } else {
        Err("unsupported format (choose from: png, svg, rm)".into())
    }
}
pub fn index(value: &str) -> std::result::Result<usize, String> {
    let (negative, value) = value
        .strip_prefix('-')
        .map_or((false, value), |v| (true, v));
    let value = if negative {
        value
    } else {
        value.strip_prefix('+').unwrap_or(value)
    };
    let (radix, digits) = if let Some(value) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (16, value)
    } else if let Some(value) = value
        .strip_prefix("0b")
        .or_else(|| value.strip_prefix("0B"))
    {
        (2, value)
    } else if let Some(value) = value
        .strip_prefix("0o")
        .or_else(|| value.strip_prefix("0O"))
    {
        (8, value)
    } else if value.len() > 1 && value.starts_with('0') {
        (8, &value[1..])
    } else {
        (10, value)
    };
    if digits.is_empty()
        || digits.ends_with('_')
        || digits.contains("__")
        || (radix == 10 && digits.starts_with('_'))
    {
        return Err("page index must be a nonnegative integer".into());
    }
    let digits = digits.replace('_', "");
    if !digits.starts_with(|ch: char| ch.is_ascii_hexdigit()) {
        return Err("page index must be a nonnegative integer".into());
    }
    usize::from_str_radix(&digits, radix)
        .ok()
        .filter(|index| *index <= isize::MAX as usize && (!negative || *index == 0))
        .ok_or_else(|| "page index must be a nonnegative integer".into())
}
pub fn safe_document_dir(title: &str, id: &str) -> String {
    let value = title.replace(['/', '\\'], "-").trim().to_string();
    if value.is_empty() || value == "." || value == ".." {
        id.into()
    } else {
        value
    }
}
pub fn safe_native_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['\\', '\0', '\r', '\n', ':'])
        && !Path::new(value).is_absolute()
        && value
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}
