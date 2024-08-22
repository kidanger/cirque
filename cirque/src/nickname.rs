pub(crate) fn cure_nickname(nickname: &str) -> Option<String> {
    decancer::cure!(nickname).map(|s| s.into()).ok()
}
