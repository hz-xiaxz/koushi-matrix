fn starts_char_literal(source: &str, index: usize) -> bool {
    let rest = &source[index + 1..];
    let Some(first) = rest.chars().next() else {
        return false;
    };
    let width = if first == '\\' {
        rest.find('\'')
    } else {
        Some(first.len_utf8())
    };
    width.is_some_and(|width| rest.as_bytes().get(width) == Some(&b'\''))
}

pub(super) fn item_body<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source.find(marker).expect("source item marker");
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .expect("source item opening brace");
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        if line_comment {
            line_comment = byte != b'\n';
        } else if block_comment > 0 {
            if byte == b'/' && next == Some(b'*') {
                block_comment += 1;
                index += 1;
            } else if byte == b'*' && next == Some(b'/') {
                block_comment -= 1;
                index += 1;
            }
        } else if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == delimiter {
                quote = None;
            }
        } else if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            index += 1;
        } else if byte == b'/' && next == Some(b'*') {
            block_comment = 1;
            index += 1;
        } else if byte == b'"' || (byte == b'\'' && starts_char_literal(source, index)) {
            quote = Some(byte);
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth -= 1;
            if depth == 0 {
                return &source[start..=index];
            }
        }
        index += 1;
    }
    panic!("source item closing brace")
}
