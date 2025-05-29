use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use run::{TestStatus, run_tests};
use std::env;
use std::fs::File;
use std::io::{Error as IoError, Read};
use std::mem;
use std::path::{Path, PathBuf};
mod run;

struct Config {
    root_dir: PathBuf,
    test_dir: PathBuf,
    cargo_toml_path: PathBuf,
}

/// Tests Markdown snippets in a directory.
///
/// This function takes three parameters:
///
/// * `dir`: The path to the directory containing the Markdown files to test.
/// * `cargo_toml_path`: The path to the `Cargo.toml` file for the project being tested.
/// * `test_dir`: An optional parameter specifying the test directory (default is the current working directory).
///
/// The function returns a vector of `TestStatus` values, where each status corresponds to a Markdown file in the directory.
///
/// Example usage:
/// ```rust
/// let test_statuses = test_snippets_in_dir("path/to/markdown/files", "Cargo.toml", Some("test/directory"));
/// ```
pub fn test_snippets_in_dir(
    dir: &str,
    cargo_toml_path: &str,
    test_dir: Option<&str>,
) -> Vec<TestStatus> {
    let files = markdown_files_of_directory(dir);
    test_snippets_in_files(cargo_toml_path, &files, test_dir)
}

pub fn test_snippets_in_files(
    cargo_toml_path: &str,
    files: &[PathBuf],
    test_dir: Option<&str>,
) -> Vec<TestStatus> {
    if files.is_empty() {
        return vec![];
    }

    for file in files {
        println!("cargo:rerun-if-changed={}", file.to_string_lossy());
    }

    let tests: Vec<Test> = files
        .iter()
        .flat_map(|path| extract_tests_from_file(path).unwrap_or_default())
        .collect();

    if tests.is_empty() {
        return vec![];
    }

    let root_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let test_dir_path = root_dir.join(test_dir.unwrap_or("skeptic_test"));

    let config = Config {
        cargo_toml_path: PathBuf::from(cargo_toml_path),
        root_dir,
        test_dir: test_dir_path,
    };

    run_tests(&config, tests)
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

    Ok(extract_tests_from_string(s, path.to_str().unwrap()))
}

#[derive(Debug)]
struct Test {
    text: Vec<String>,
    path: PathBuf,
    section: Option<String>,
    line_number: usize,
    ignore: bool,
    no_run: bool,
    should_panic: bool,
}

impl Test {
    pub fn name(&self) -> String {
        let file_stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut name = format!("{}_line_{}", file_stem, self.line_number);

        if let Some(ref section) = self.section {
            name = format!("{}_sect_{}", name, section);
        }

        name
    }
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
                    section = Some(sect);
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
                    buf.extend(
                        text.lines()
                            .filter_map(clean_code_line) // <- Only keep meaningful lines
                            .map(|s| format!("{}", s)),
                    );
                } else if let Buffer::Heading(ref mut buf) = buffer {
                    buf.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if current_code_block_info.is_none() {
                    continue;
                }
                if let Buffer::Code(buf) = mem::replace(&mut buffer, Buffer::None) {
                    let info = current_code_block_info.take().unwrap();

                    tests.push(Test {
                        text: buf,
                        path: file_stem.into(),
                        section: section.clone(),
                        line_number: code_block_start,
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

fn clean_code_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix("# ") {
        return Some(rest);
    } else if trimmed == "#" || trimmed.is_empty() {
        return None;
    }
    return Some(trimmed);
}
