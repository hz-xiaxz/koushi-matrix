use super::*;

#[test]
fn writes_the_report_verbatim_to_the_chosen_destination() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join(DIAGNOSTICS_FILE_NAME);
    let report = "Koushi diagnostics\npreview_truncated=true omitted_characters=12\n";

    write_diagnostics_report(&path, report).expect("report write");

    assert_eq!(
        std::fs::read_to_string(&path).expect("written report"),
        report
    );
}

#[test]
fn reports_a_write_failure_without_naming_the_destination() {
    let directory = tempfile::tempdir().expect("temp dir");
    let missing_parent = directory.path().join("missing").join("report.txt");

    let error = write_diagnostics_report(&missing_parent, "Koushi diagnostics\n")
        .expect_err("write into a missing directory fails");

    assert_eq!(error, "could not write the diagnostics report");
    assert!(!error.contains("missing"));
}

#[test]
fn offers_a_plain_text_default_file_name() {
    assert_eq!(DIAGNOSTICS_FILE_NAME, "koushi-diagnostics.txt");
}
