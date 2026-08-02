/// Pop the next newline-terminated line off `buf`, stripping the trailing
/// `\n` (and `\r` if present). Returns `None` when `buf` has no complete
/// line yet.
pub fn next_line(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let nl = buf.iter().position(|&b| b == b'\n')?;
    let mut s: Vec<u8> = buf.drain(..=nl).collect();
    if s.last() == Some(&b'\n') {
        s.pop();
    }
    if s.last() == Some(&b'\r') {
        s.pop();
    }
    Some(s)
}

/// Return the un-terminated tail of `buf` (after stripping any trailing
/// `\r`/`\n`) if non-empty. Use after the stream has closed to recover a
/// final line emitted without a trailing newline — the server 0.24+ has been
/// observed to do this on `/api/create` and `/api/pull`, which would
/// otherwise cause a successful install to surface as "stream ended
/// without success".
pub fn tail(buf: &[u8]) -> Option<&[u8]> {
    let mut s = buf;
    while matches!(s.last(), Some(b'\n') | Some(b'\r')) {
        s = &s[..s.len() - 1];
    }
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_line_pops_one_complete_line() {
        let mut b: Vec<u8> = b"a\nbc\nd".to_vec();
        assert_eq!(next_line(&mut b).as_deref(), Some(b"a".as_ref()));
        assert_eq!(next_line(&mut b).as_deref(), Some(b"bc".as_ref()));
        assert_eq!(next_line(&mut b), None);
        assert_eq!(b, b"d");
    }

    #[test]
    fn next_line_strips_crlf() {
        let mut b: Vec<u8> = b"hi\r\n".to_vec();
        assert_eq!(next_line(&mut b).as_deref(), Some(b"hi".as_ref()));
    }

    #[test]
    fn tail_returns_unterminated_remainder() {
        assert_eq!(tail(b"abc"), Some(b"abc".as_ref()));
        assert_eq!(tail(b"abc\n"), Some(b"abc".as_ref()));
        assert_eq!(tail(b"abc\r\n"), Some(b"abc".as_ref()));
        assert_eq!(tail(b""), None);
        assert_eq!(tail(b"\n\r\n"), None);
    }
}
