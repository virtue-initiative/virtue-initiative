pub const BUILD_LABEL: &str = env!("VIRTUE_BUILD_LABEL");

pub fn build_label() -> &'static str {
    BUILD_LABEL
}
