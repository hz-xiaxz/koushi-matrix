use koushi_protocol::failure::ReportFailureKind;

pub(crate) fn classify_report_error(error: &koushi_sdk::MatrixReportError) -> ReportFailureKind {
    use koushi_sdk::MatrixReportFailureKind;

    match error.failure_kind() {
        MatrixReportFailureKind::Forbidden => ReportFailureKind::Forbidden,
        MatrixReportFailureKind::Network => ReportFailureKind::Network,
        MatrixReportFailureKind::InvalidUserId => ReportFailureKind::InvalidUserId,
        MatrixReportFailureKind::InvalidRoomId => ReportFailureKind::InvalidRoomId,
        MatrixReportFailureKind::InvalidEventId => ReportFailureKind::InvalidEventId,
        MatrixReportFailureKind::Sdk => ReportFailureKind::Sdk,
    }
}
