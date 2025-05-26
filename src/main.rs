use skeptic_rs::markdown_files_of_directory;

pub fn main() {
    let paths = markdown_files_of_directory("./book");

    // for path in &paths {
    //     extract_tests_from_file(path);
    // }
}
