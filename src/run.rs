use crate::{Config, Test};
use std::fmt::Display;
use std::fs::{copy, create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn initialize_test(config: &Config) -> (PathBuf, PathBuf) {
    let test_dir = &config.test_dir;
    
    let src_dir = test_dir.join("src");
    let main_file = src_dir.join("main.rs");

    create_dir_all(&src_dir).unwrap();

    let cargo_toml_src = &config.root_dir.join("Cargo.toml");
    let cargo_toml_dst = test_dir.join("Cargo.toml");
    copy(&cargo_toml_src, &cargo_toml_dst).unwrap();

    (test_dir.to_path_buf(), main_file)
}

pub fn run_tests(config: &Config, tests: Vec<Test>) {
    let (test_dir, main_file) = initialize_test(config);

    // let test_results = tests
    //     .iter()
    //     .map(|test| run_test(&test_dir, &main_file, test))
    //     .collect::<Vec<_>>();


    let mut results = Vec::with_capacity(tests.len());

    for test in &tests {
        let status = run_test(&test_dir, &main_file, test);
        println!("{} {}", test.name, status_print(&status));
        results.push(status);
    }

    if let Err(err) = remove_dir_all(&test_dir) {
        eprintln!(
            "Warning: Failed to remove test directory {}: {}",
            test_dir.display(),
            err
        );
    }

    print_test_stats(&results);
}

enum TestStatus {
    Ignored,
    Passed,
    Failed,
}

fn print_test_stats(results: &Vec<TestStatus>) {
    use ansi_term::Color;
    let mut passed = 0;
    let mut failed = 0;
    let mut ignored = 0;
    for result in results {
        match result {
            TestStatus::Passed => passed += 1,
            TestStatus::Failed => failed += 1,
            TestStatus::Ignored => ignored += 1,
        }
    }

    println!("\n{}", Color::Cyan.paint("Test Summary:"));
    println!("  ✅ Passed : {}", Color::Green.paint(passed.to_string()));
    println!("  ❌ Failed : {}", Color::Red.paint(failed.to_string()));
    println!("  ⚠️ Ignored: {}", Color::Yellow.paint(ignored.to_string()));
    println!("  📦 Total  : {}", results.len().to_string());
}

fn status_print(status: &TestStatus) -> impl Display {
    use ansi_term::Color;
    match status {
        TestStatus::Passed => Color::Green.paint("passed"),
        TestStatus::Failed => Color::Red.paint("failed"),
        TestStatus::Ignored => Color::Yellow.paint("ignored"),
    }
}

fn run_test(test_dir: &Path, main_file: &Path, test: &Test) -> TestStatus {
    if test.ignore {
        println!("Ignoring test: {}", test.name);
        return TestStatus::Ignored;
    }

    write(main_file, test.text.join("\n")).unwrap();

    if test.no_run {
        println!("Checking (no_run): {}", test.name);
        let status = Command::new("cargo")
            .arg("check")
            .current_dir(test_dir)
            .status()
            .expect("Failed to run cargo check");

        if !status.success() {
            eprintln!("(no_run) test {} failed to compile.", test.name);
            return TestStatus::Failed;
        }
        return TestStatus::Passed;
    }

    let output = Command::new("cargo")
        .arg("run")
        .current_dir(test_dir)
        .output()
        .expect("Failed to execute test");

    let success = output.status.success();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if test.should_panic {
        if success {
            println!("Test '{}' was expected to panic but passed.", test.name);
            return TestStatus::Failed;
        } else {
            return TestStatus::Passed;
        }
    } else {
        if !success {
            eprintln!(
                "Test '{}' failed.\nstdout:\n{}\nstderr:\n{}",
                test.name, stdout, stderr
            );
            return TestStatus::Failed;
        } else {
            return TestStatus::Passed;
        }
    }
}
