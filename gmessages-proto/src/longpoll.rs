//! Incremental parser of the event stream body (`[[ item , item , … ]]`, spec §5).

/// Error of the stream body parser.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LongPollError {
    /// An unexpected byte outside of an item.
    #[error("unexpected byte {0:#04x} at offset {1}")]
    Unexpected(u8, usize),
    /// An item exceeded the size limit.
    #[error("item larger than {0} bytes")]
    TooLarge(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Before `[[`: number of `[` seen.
    Start(u8),
    /// Between items.
    Between,
    /// Inside an item.
    Item,
    /// After `]]`.
    Done,
}

/// Splits the streamed body into complete JSON items, whatever the read boundaries.
///
/// After the opening `[[`, each item is one complete JSON array, found by tracking bracket
/// depth, strings and escapes; separators (`,`, whitespace) are skipped; `]]` ends the body.
#[derive(Debug)]
pub struct LongPollParser {
    state: State,
    item: Vec<u8>,
    depth: usize,
    in_str: bool,
    esc: bool,
    offset: usize,
    max_item: usize,
}

impl Default for LongPollParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LongPollParser {
    /// Default limit on the size of one item (16 MiB).
    pub const DEFAULT_MAX_ITEM: usize = 16 << 20;

    /// A parser with the default item size limit.
    pub fn new() -> Self {
        Self::with_max_item(Self::DEFAULT_MAX_ITEM)
    }

    /// A parser with a given item size limit.
    pub fn with_max_item(max_item: usize) -> Self {
        LongPollParser {
            state: State::Start(0),
            item: Vec::new(),
            depth: 0,
            in_str: false,
            esc: false,
            offset: 0,
            max_item,
        }
    }

    /// Whether the closing `]]` was seen.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Feeds bytes; returns the items completed by them (each a JSON array as bytes).
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, LongPollError> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            let at = self.offset + i;
            match self.state {
                State::Start(n) => {
                    if b == b'[' {
                        self.state = if n == 1 {
                            State::Between
                        } else {
                            State::Start(1)
                        };
                    } else if !b.is_ascii_whitespace() {
                        return Err(LongPollError::Unexpected(b, at));
                    }
                }
                State::Between => match b {
                    b'[' => {
                        self.state = State::Item;
                        self.depth = 1;
                        self.in_str = false;
                        self.esc = false;
                        self.item.clear();
                        self.item.push(b);
                    }
                    b']' => {
                        // end of the item list; the outer `]` follows
                        self.state = State::Done;
                    }
                    b',' => {}
                    b if b.is_ascii_whitespace() => {}
                    b => return Err(LongPollError::Unexpected(b, at)),
                },
                State::Item => {
                    // Copy a run of bytes up to the next interesting one.
                    let start = i;
                    let mut j = i;
                    let mut finished = false;
                    while j < data.len() {
                        let c = data[j];
                        j += 1;
                        if self.in_str {
                            if self.esc {
                                self.esc = false;
                            } else if c == b'\\' {
                                self.esc = true;
                            } else if c == b'"' {
                                self.in_str = false;
                            }
                        } else {
                            match c {
                                b'"' => self.in_str = true,
                                b'[' | b'{' => self.depth += 1,
                                b']' | b'}' => {
                                    self.depth -= 1;
                                    if self.depth == 0 {
                                        finished = true;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    self.item.extend_from_slice(&data[start..j]);
                    if self.item.len() > self.max_item {
                        return Err(LongPollError::TooLarge(self.max_item));
                    }
                    if finished {
                        out.push(std::mem::take(&mut self.item));
                        self.state = State::Between;
                    }
                    i = j;
                    continue;
                }
                State::Done => {}
            }
            i += 1;
        }
        self.offset += data.len();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"[[[null,null,null,null,[]],[null,null,null,[3]],[null,["id-1",19,null,null,2,null,null,null,null,null,null,"QUJD"]],
 [null,["id-2",19,"x","a \"quoted\" ]] [[ string \\ with \\\" escapes ]",{"k":"]"}]],[null,null,[]]]]"#;

    fn expected() -> Vec<String> {
        vec![
            "[null,null,null,null,[]]".into(),
            "[null,null,null,[3]]".into(),
            r#"[null,["id-1",19,null,null,2,null,null,null,null,null,null,"QUJD"]]"#.into(),
            r#"[null,["id-2",19,"x","a \"quoted\" ]] [[ string \\ with \\\" escapes ]",{"k":"]"}]]"#.into(),
            "[null,null,[]]".into(),
        ]
    }

    fn run(splits: &[usize]) -> (Vec<String>, bool) {
        let b = BODY.as_bytes();
        let mut p = LongPollParser::new();
        let mut items = Vec::new();
        let mut last = 0;
        for &s in splits.iter().chain(std::iter::once(&b.len())) {
            for it in p.feed(&b[last..s]).unwrap() {
                items.push(String::from_utf8(it).unwrap());
            }
            last = s;
        }
        (items, p.is_done())
    }

    #[test]
    fn whole_body() {
        let (items, done) = run(&[]);
        assert_eq!(items, expected());
        assert!(done);
        for i in &items {
            serde_json::from_str::<serde_json::Value>(i).unwrap();
        }
    }

    #[test]
    fn every_single_split_and_byte_by_byte() {
        let n = BODY.len();
        for s in 0..=n {
            assert_eq!(run(&[s]).0, expected(), "split at {s}");
        }
        let all: Vec<usize> = (1..n).collect();
        let (items, done) = run(&all);
        assert_eq!(items, expected());
        assert!(done);
    }

    #[test]
    fn pseudo_random_splits() {
        let n = BODY.len();
        let mut seed = 0x1234_5678u64;
        for _ in 0..200 {
            let mut cuts = Vec::new();
            let mut pos = 0;
            loop {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                pos += 1 + (seed >> 33) as usize % 17;
                if pos >= n {
                    break;
                }
                cuts.push(pos);
            }
            assert_eq!(run(&cuts).0, expected());
        }
    }

    #[test]
    fn errors_and_limits() {
        let mut p = LongPollParser::new();
        assert!(p.feed(b"{").is_err());
        let mut p = LongPollParser::with_max_item(10);
        assert_eq!(
            p.feed(b"[[[1,2,3,4,5,6,7,8]]]"),
            Err(LongPollError::TooLarge(10))
        );
        let mut p = LongPollParser::new();
        assert_eq!(p.feed(b" [[ ]]").unwrap().len(), 0);
        assert!(p.is_done());
    }
}
