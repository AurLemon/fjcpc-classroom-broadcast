/// Sanitize a filename for use on Windows filesystems by replacing reserved
/// characters with underscores and trimming trailing dots/spaces.
pub fn sanitize_filename(input: &str) -> String {
    const INVALID: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let mut sanitized = String::with_capacity(input.len());
    for ch in input.chars() {
        if INVALID.contains(&ch) || ch.is_control() {
            sanitized.push('_');
        } else {
            sanitized.push(ch);
        }
    }
    let sanitized = sanitized.trim().trim_matches('.').to_string();
    if sanitized.is_empty() {
        "_".to_string()
    } else {
        sanitized
    }
}
