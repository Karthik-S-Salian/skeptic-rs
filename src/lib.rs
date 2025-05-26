use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use std::env;
use std::fs::File;
use std::io::{Error as IoError, Read};
use std::mem;
use std::path::{Path, PathBuf};

struct Config {
    root_dir: PathBuf,
    tests: Vec<Test>,
}

pub fn test_snippets_in_dir(dir: &str) {
    let files = markdown_files_of_directory(dir);

    if files.is_empty() {
        return;
    }

    // Inform cargo that it needs to rerun the build script if one of the skeptic files are
    // modified
    for doc in &files {
        println!("cargo:rerun-if-changed={}", doc.to_string_lossy());
    }

    let tests = files
        .iter()
        .map(|path| extract_tests_from_file(path).unwrap())
        .flatten()
        .collect::<Vec<_>>();

    let root_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let config = Config { root_dir, tests };
}

pub fn markdown_files_of_directory(dir: &str) -> Vec<PathBuf> {
    use glob::{MatchOptions, glob_with};

    let opts = MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let mut out = Vec::new();

    for path in glob_with(&format!("{}/**/*.md", dir), opts)
        .expect("Failed to read glob pattern")
        .filter_map(Result::ok)
    {
        out.push(path.to_str().unwrap().into());
    }

    out
}

fn extract_tests_from_file(path: &Path) -> Result<Vec<Test>, IoError> {
    let mut file = File::open(path)?;
    let s = &mut String::new();
    file.read_to_string(s)?;

    let file_stem = &sanitize_test_name(path.to_str().unwrap());

    Ok(extract_tests_from_string(s, file_stem))
}

#[derive(Debug)]
struct Test {
    name: String,
    text: Vec<String>,
    ignore: bool,
    no_run: bool,
    should_panic: bool,
}

enum Buffer {
    None,
    Code(Vec<String>),
    Heading(String),
}

fn extract_tests_from_string(s: &str, file_stem: &str) -> Vec<Test> {
    let mut tests = Vec::new();
    let mut buffer = Buffer::None;
    let parser = Parser::new(s);
    let mut section = None;
    let mut code_block_start = 0;

    let mut current_code_block_info: Option<CodeBlockInfo> = None;

    for (event, range) in parser.into_offset_iter() {
        let line_number = bytecount::count(&s.as_bytes()[0..range.start], b'\n');
        match event {
            Event::Start(Tag::Heading { level, .. }) if level < HeadingLevel::H3 => {
                buffer = Buffer::Heading(String::new());
            }
            Event::End(TagEnd::Heading(level)) if level < HeadingLevel::H3 => {
                let cur_buffer = mem::replace(&mut buffer, Buffer::None);
                if let Buffer::Heading(sect) = cur_buffer {
                    section = Some(sanitize_test_name(&sect));
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info))) => {
                let code_block_info = parse_code_block_info(info);
                if code_block_info.is_rust {
                    buffer = Buffer::Code(Vec::new());
                    current_code_block_info = Some(code_block_info);
                }
            }
            Event::Text(text) => {
                if let Buffer::Code(ref mut buf) = buffer {
                    if buf.is_empty() {
                        code_block_start = line_number;
                    }
                    buf.extend(text.lines().map(|s| format!("{}\n", s)));
                } else if let Buffer::Heading(ref mut buf) = buffer {
                    buf.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if current_code_block_info.is_none() {
                    continue;
                }
                if let Buffer::Code(buf) = mem::replace(&mut buffer, Buffer::None) {
                    let name = if let Some(ref section) = section {
                        format!("{}_sect_{}_line_{}", file_stem, section, code_block_start)
                    } else {
                        format!("{}_line_{}", file_stem, code_block_start)
                    };

                    let info = current_code_block_info.take().unwrap();

                    tests.push(Test {
                        name,
                        text: buf,
                        ignore: info.ignore,
                        no_run: info.no_run,
                        should_panic: info.should_panic,
                    });
                }
            }
            _ => (),
        }
    }
    tests
}

fn sanitize_test_name(s: &str) -> String {
    s[..s.len() - 3]
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii() && ch.is_alphanumeric() {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

struct CodeBlockInfo {
    is_rust: bool,
    should_panic: bool,
    ignore: bool,
    no_run: bool,
}

fn parse_code_block_info(info: &str) -> CodeBlockInfo {
    let tokens = info.split(|c: char| !(c == '_' || c == '-' || c.is_alphanumeric()));

    let mut seen_rust_tags = false;
    let mut seen_other_tags = false;
    let mut info = CodeBlockInfo {
        is_rust: false,
        should_panic: false,
        ignore: false,
        no_run: false,
    };

    for token in tokens {
        match token {
            "" => {}
            "rust" => {
                info.is_rust = true;
                seen_rust_tags = true
            }
            "should_panic" => {
                info.should_panic = true;
                seen_rust_tags = true
            }
            "ignore" => {
                info.ignore = true;
                seen_rust_tags = true
            }
            "no_run" => {
                info.no_run = true;
                seen_rust_tags = true;
            }
            _ => seen_other_tags = true,
        }
    }

    info.is_rust &= !seen_other_tags || seen_rust_tags;

    info
}
